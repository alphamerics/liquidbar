//! The global "disable the taskbar" toggle shortcut.
//!
//! Registers a system-wide hotkey (`RegisterHotKey`) on a dedicated
//! message-pumping thread — the same pattern as `wheel_volume`'s low-level
//! hook thread. Pressing the combo toggles the disabled state: while
//! disabled, the supervision loop (hit_test_loop) forces the taskbar into
//! the Hidden layer no matter where the cursor is, so the pill is
//! completely gone until the combo is pressed again.
//!
//! The combo is a user setting (`disableHotkey`, stored as a "+"-joined
//! token string like "Ctrl+Alt+Shift+H"). Changing it posts `WM_APP` to
//! this thread, which re-registers; if Windows refuses (the combo is
//! already claimed by another application), the previous combo is retried
//! before giving up. Failures are logged and swallowed: everything else
//! keeps working.
//!
//! The disabled state is deliberately NOT persisted: it is a transient
//! "get it off my screen" switch that always resets to enabled on the next
//! launch.

use crate::settings::GlassbarSettings;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PostThreadMessageW, MSG, WM_APP, WM_HOTKEY,
};

/// The registration id (this process only ever registers one hotkey).
const HOTKEY_ID: i32 = 0x4742;
/// The re-registration message posted to the hotkey thread when the
/// setting changes.
const WM_REREGISTER: u32 = WM_APP + 1;

/// The live disabled state (see the module docs).
static DISABLED: AtomicBool = AtomicBool::new(false);
/// The hotkey thread's id, so [`reapply`] can post it a re-registration.
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// Whether the taskbar is currently disabled by the toggle hotkey. Read
/// every supervision tick.
pub fn taskbar_disabled() -> bool {
    DISABLED.load(Ordering::Relaxed)
}

/// Spawns the hotkey thread: registers the saved combo and pumps messages
/// until the process exits.
pub fn spawn_hotkey_service(app: tauri::AppHandle) {
    std::thread::spawn(move || hotkey_thread(app));
}

/// Asks the hotkey thread to re-register with the current setting. A
/// no-op before the service has spawned (the thread registers itself from
/// the settings at boot).
pub fn reapply() {
    let thread = THREAD_ID.load(Ordering::SeqCst);
    if thread != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread, WM_REREGISTER, WPARAM(0), LPARAM(0));
        }
    }
}

fn hotkey_thread(app: tauri::AppHandle) {
    THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    let mut live = register_from_settings(&app, None);
    // WM_HOTKEY is posted to the thread's queue (no window is needed when
    // the registration passes no HWND), as is the WM_APP re-registration.
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        match message.message {
            WM_HOTKEY if message.wParam.0 as i32 == HOTKEY_ID => toggle(&app),
            WM_REREGISTER => {
                if live.is_some() {
                    unsafe {
                        let _ = UnregisterHotKey(None, HOTKEY_ID);
                    }
                }
                live = register_from_settings(&app, live);
            }
            _ => {}
        }
    }
    if live.is_some() {
        unsafe {
            let _ = UnregisterHotKey(None, HOTKEY_ID);
        }
    }
}

/// Registers the setting's combo (falling back to the previously live one
/// when the new combo is refused, e.g. claimed by another application).
/// Returns the combo now live, if any.
fn register_from_settings(
    app: &tauri::AppHandle,
    fallback: Option<(HOT_KEY_MODIFIERS, u32)>,
) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let settings_state = app.state::<Mutex<GlassbarSettings>>();
    let guard = settings_state.lock().expect("settings lock poisoned");
    let requested = parse_hotkey(&guard.disable_hotkey);
    for (modifiers, key, label) in requested
        .iter()
        .map(|&(modifiers, key)| (modifiers, key, "requested"))
        .chain(
            fallback
                .iter()
                .map(|&(modifiers, key)| (modifiers, key, "previous")),
        )
    {
        if unsafe { RegisterHotKey(None, HOTKEY_ID, modifiers, key) }.is_ok() {
            eprintln!("[hotkey] toggle hotkey registered ({label})");
            return Some((modifiers, key));
        }
    }
    eprintln!("[hotkey] toggle hotkey registration failed (in use elsewhere?)");
    None
}

fn toggle(app: &tauri::AppHandle) {
    // The hotkey thread is the only writer, so load/store cannot race.
    let now_disabled = !DISABLED.load(Ordering::Relaxed);
    DISABLED.store(now_disabled, Ordering::Relaxed);
    eprintln!("[hotkey] taskbar disabled={now_disabled}");
    let _ = app.emit("taskbar-disabled", now_disabled);
}

/// Parses a "+"-joined combo string ("Ctrl+Alt+Shift+H") into the
/// registration's modifiers and virtual key. `MOD_NOREPEAT` is always
/// included, so holding the combo toggles once.
fn parse_hotkey(text: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key = None;
    for token in text
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "windows" | "meta" | "super" => modifiers |= MOD_WIN,
            other => key = Some(parse_key(other)?),
        }
    }
    key.map(|key| (modifiers, key))
}

/// Maps one key token (lowercase, e.g. "h", "f5", "space") to its virtual
/// key code.
fn parse_key(token: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 {
        let ch = bytes[0];
        // Letters and digits share their ASCII codes as virtual keys.
        if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
            return Some(ch as u32);
        }
        // Punctuation maps to the OEM keys.
        return match ch {
            b';' => Some(0xBA),
            b'=' => Some(0xBB),
            b',' => Some(0xBC),
            b'-' => Some(0xBD),
            b'.' => Some(0xBE),
            b'/' => Some(0xBF),
            b'`' => Some(0xC0),
            b'[' => Some(0xDB),
            b'\\' => Some(0xDC),
            b']' => Some(0xDD),
            b'\'' => Some(0xDE),
            _ => None,
        };
    }
    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Some(0x70 + n - 1);
            }
        }
    }
    Some(match upper.as_str() {
        "SPACE" => 0x20,
        "TAB" => 0x09,
        "ENTER" | "RETURN" => 0x0D,
        "BACKSPACE" => 0x08,
        "ESCAPE" | "ESC" => 0x1B,
        "DELETE" | "DEL" => 0x2E,
        "INSERT" | "INS" => 0x2D,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        "PRINTSCREEN" | "PRTSC" => 0x2C,
        _ => return None,
    })
}
