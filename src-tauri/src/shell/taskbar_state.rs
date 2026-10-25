//! Hiding Windows' own taskbar (Explorer's `Shell_TrayWnd` windows) so
//! Glassbar fully replaces it, ported from amnweb/thide: `ShowWindow(SW_HIDE)`
//! on every Explorer-owned taskbar window, plus the `ABS_AUTOHIDE` AppBar
//! state (Explorer writes that state back when it re-renders the taskbar), a
//! light monitor loop that re-hides a taskbar something made visible again,
//! and restoration of the original AppBar state on show/exit.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOW,
};

/// How often the monitor loop re-hides a taskbar something made visible.
const TASKBAR_MONITOR_INTERVAL_MS: u64 = 100;

/// Sentinel for "the taskbar's original AppBar state was never captured".
const APPBAR_STATE_UNSET: u32 = u32::MAX;

/// Whether the user wants the Windows taskbar hidden (the settings mirror).
static HIDE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The AppBar state the taskbar had before Glassbar first hid it, restored
/// on show/exit so the user's auto-hide preference survives the session.
static ORIGINAL_APPBAR_STATE: AtomicU32 = AtomicU32::new(APPBAR_STATE_UNSET);

/// The owning process name for a window ("explorer.exe" for real taskbars).
fn process_name_for_window(hwnd: HWND) -> Option<String> {
    let mut process_id: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if process_id == 0 {
        return None;
    }
    let wide = super::icons::executable_path(process_id)?;
    let path = String::from_utf16_lossy(&wide[..wide.len().saturating_sub(1)]);
    std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Every Explorer-owned taskbar window: the primary taskbar
/// (`Shell_TrayWnd`) and one secondary taskbar per extra monitor
/// (`Shell_SecondaryTrayWnd`). Only windows owned by explorer.exe count —
/// applications sometimes reuse the class names.
fn find_all_explorer_taskbars() -> Vec<HWND> {
    let mut taskbars = Vec::new();
    for class_name in ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
        let mut class_wide: Vec<u16> = class_name.encode_utf16().collect();
        class_wide.push(0);
        let mut hwnd = HWND::default();
        loop {
            let found = unsafe {
                FindWindowExW(
                    HWND::default(),
                    hwnd,
                    windows::core::PCWSTR(class_wide.as_ptr()),
                    windows::core::PCWSTR::null(),
                )
            }
            .unwrap_or(HWND::default());
            if found.0.is_null() {
                break;
            }
            hwnd = found;
            if process_name_for_window(found)
                .map(|name| name.eq_ignore_ascii_case("explorer.exe"))
                .unwrap_or(false)
            {
                taskbars.push(found);
                if class_name == "Shell_TrayWnd" {
                    // Only one primary taskbar exists.
                    break;
                }
            }
        }
    }
    taskbars
}

/// Whether any Explorer taskbar is currently on screen.
pub fn is_taskbar_visible() -> bool {
    find_all_explorer_taskbars()
        .into_iter()
        .any(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
}

/// Shows or hides every Explorer taskbar window.
fn set_taskbar_windows_visible(show: bool) {
    let command = if show { SW_SHOW } else { SW_HIDE };
    for hwnd in find_all_explorer_taskbars() {
        unsafe {
            let _ = ShowWindow(hwnd, command);
        }
    }
}

/// Reads the taskbar's current AppBar state (auto-hide flag included).
fn read_appbar_state() -> u32 {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 }
}

/// Writes the taskbar's AppBar state.
fn write_appbar_state(state: u32) {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        lParam: LPARAM(state as isize),
        ..Default::default()
    };
    unsafe {
        let _ = SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
}

/// Captures the taskbar's AppBar state once per session, then forces
/// auto-hide on top of it and hides every taskbar window.
fn hide_taskbar() {
    let original = ORIGINAL_APPBAR_STATE.load(Ordering::Relaxed);
    if original == APPBAR_STATE_UNSET {
        let current = read_appbar_state();
        ORIGINAL_APPBAR_STATE.store(current, Ordering::Relaxed);
        let enforced = current | ABS_AUTOHIDE;
        if enforced != current {
            write_appbar_state(enforced);
        }
    }
    set_taskbar_windows_visible(false);
}

/// Restores the taskbar's original AppBar state and shows its windows.
fn show_taskbar() {
    let original = ORIGINAL_APPBAR_STATE.swap(APPBAR_STATE_UNSET, Ordering::Relaxed);
    if original != APPBAR_STATE_UNSET {
        write_appbar_state(original);
    }
    set_taskbar_windows_visible(true);
}

/// Applies the "hide the Windows taskbar" setting: hides (or shows and
/// restores) the taskbar immediately. Idempotent — safe to call on every
/// settings change.
pub fn set_taskbar_hidden(hide: bool) {
    HIDE_REQUESTED.store(hide, Ordering::SeqCst);
    if hide {
        hide_taskbar();
    } else {
        show_taskbar();
    }
}

/// Shows the taskbar and restores its original AppBar state. Called on exit
/// so Glassbar never leaves the user's taskbar gone.
pub fn restore_taskbar() {
    set_taskbar_hidden(false);
}

/// Starts the monitor loop that re-hides a taskbar something made visible
/// again (Explorer re-shows its taskbar windows on shell events even while
/// they are hidden). Runs for the process lifetime.
pub fn start_taskbar_monitor() {
    std::thread::spawn(move || loop {
        if HIDE_REQUESTED.load(Ordering::SeqCst) && is_taskbar_visible() {
            set_taskbar_windows_visible(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(
            TASKBAR_MONITOR_INTERVAL_MS,
        ));
    });
}
