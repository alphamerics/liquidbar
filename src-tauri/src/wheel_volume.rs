//! Scroll-wheel volume on the taskbar pill.
//!
//! A low-level mouse hook (`WH_MOUSE_LL`) watches wheel events globally.
//! While the cursor is over the taskbar pill's band, the wheel event is
//! swallowed and turned into a master-volume step on a worker thread; over
//! anything else the event passes through untouched. The hook only ever
//! consumes `WM_MOUSEWHEEL` — clicks, drags and the click-through hit
//! testing are never touched, so the pill's empty areas keep passing input
//! to the windows behind.
//!
//! The hook refuses the wheel when the pill is not actually on top
//! (desired_topmost false: parked beneath a window, settings open) or the
//! cursor is over an open panel — those scrolls belong to whatever is really
//! under the cursor. Notches coalesce on the worker, so a fast spin reads
//! the master volume once per batch instead of racing itself through
//! per-notch read-modify-writes.

use crate::renderer::RendererState;
use crate::shell;
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;
use tauri::Manager;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, GetWindowRect, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
    MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEWHEEL,
};

/// One wheel notch's volume step (Windows' own tray behavior steps 2%).
const VOLUME_STEP: f64 = 0.02;
/// How long the worker waits after the first notch before applying, so a
/// fast spin's notches land in one batch.
const COALESCE: Duration = Duration::from_millis(24);

thread_local! {
    /// The hook proc carries no context, and low-level hooks run on the
    /// installing thread — thread-locals are the context channel.
    static WHEEL_TX: RefCell<Option<mpsc::Sender<i32>>> = const { RefCell::new(None) };
    static CONTEXT: RefCell<Option<(tauri::AppHandle, isize)>> = const { RefCell::new(None) };
}

/// Installs the hook thread and the volume worker. Failures are logged and
/// swallowed: the feature is off, everything else keeps working.
pub fn spawn_wheel_volume(app: tauri::AppHandle, hwnd: isize) {
    let (tx, rx) = mpsc::channel::<i32>();
    std::thread::spawn(move || volume_worker(rx));
    std::thread::spawn(move || hook_thread(app, hwnd, tx));
}

fn hook_thread(app: tauri::AppHandle, hwnd: isize, tx: mpsc::Sender<i32>) {
    CONTEXT.with(|cell| *cell.borrow_mut() = Some((app, hwnd)));
    WHEEL_TX.with(|cell| *cell.borrow_mut() = Some(tx));
    unsafe {
        let hook: HHOOK =
            match SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_hook_proc), HINSTANCE::default(), 0) {
                Ok(hook) => hook,
                Err(error) => {
                    eprintln!("[wheel-volume] hook installation failed: {error}");
                    return;
                }
            };
        eprintln!("[wheel-volume] hook installed");
        // A low-level hook only fires while its thread pumps messages.
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                break;
            }
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn wheel_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == WM_MOUSEWHEEL as usize {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        // The delta lives in the upper word (positive = rolled up).
        let delta = (info.mouseData >> 16) as i16 as i32;
        if delta != 0 {
            let take = CONTEXT.with(|cell| {
                let borrowed = cell.borrow();
                let Some((app, hwnd)) = borrowed.as_ref() else {
                    return false;
                };
                wheel_over_pill(info.pt, app, HWND(*hwnd as *mut _))
            });
            if take {
                WHEEL_TX.with(|cell| {
                    if let Some(tx) = cell.borrow().as_ref() {
                        let _ = tx.send(delta.signum());
                    }
                });
                // Swallow: the scroll belongs to the pill now.
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Whether a wheel event at `pt` should become a volume step.
unsafe fn wheel_over_pill(pt: POINT, app: &tauri::AppHandle, overlay: HWND) -> bool {
    let mut rect = RECT::default();
    if GetWindowRect(overlay, &mut rect).is_err() {
        return false;
    }
    let inside = pt.x >= rect.left && pt.x < rect.right && pt.y >= rect.top && pt.y < rect.bottom;
    if !inside {
        return false;
    }
    // Only the pill's own strip: the overlay's taller body hosts the
    // floating panels, whose scroll belongs to them (the panels never reach
    // down into the pill band, so the band check alone mostly decides).
    if pt.y < rect.bottom - shell::TASKBAR_HEIGHT_PX {
        return false;
    }
    let state = app.state::<RendererState>();
    // The pill must be on top: a desktop-parked pill (or one under the
    // settings window) must never steal a scroll meant for the window
    // covering it.
    if state.settings_open() || !state.desired_topmost() {
        return false;
    }
    // An open panel (control center, context menu, preview) owns its area.
    !state
        .open_panel_hit_rects()
        .iter()
        .any(|hit| pt.x >= hit[0] && pt.x < hit[2] && pt.y >= hit[1] && pt.y < hit[3])
}

/// The volume worker: batches the hook's notches and applies them as one
/// read-modify-write against the live master volume. Raising from muted
/// unmutes (Windows' own tray behavior); lowering keeps the mute.
fn volume_worker(rx: mpsc::Receiver<i32>) {
    while let Ok(first) = rx.recv() {
        std::thread::sleep(COALESCE);
        let mut steps = first;
        while let Ok(more) = rx.try_recv() {
            steps += more;
        }
        apply_volume_step(steps);
    }
}

fn apply_volume_step(steps: i32) {
    let delta = steps as f64 * VOLUME_STEP;
    let Ok((master, muted)) = shell::audio_master() else {
        return;
    };
    let next = (master + delta).clamp(0.0, 1.0);
    if let Err(error) = shell::set_audio_master_volume(next) {
        eprintln!("[wheel-volume] set failed: {error}");
        return;
    }
    if muted && delta > 0.0 {
        let _ = shell::set_audio_muted(false);
    }
}
