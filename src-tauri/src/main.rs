// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

/// Name of the session-local mutex marking a running Glassbar. A second
/// launch detects the same name and exits, so `glassbar.exe` (double-click,
/// autostart, shortcuts) can never spawn a second taskbar.
const SINGLE_INSTANCE_MUTEX: &str = "Glassbar.SingleInstance";

/// Encodes a string as the NUL-terminated UTF-16 the Win32 API expects.
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Takes the single-instance lock. Returns `false` when another Glassbar in
/// this session already owns it. Failing to create the mutex fails open: a
/// broken lock must not brick the taskbar itself.
fn acquire_single_instance() -> bool {
    unsafe {
        let name = to_wide(SINGLE_INSTANCE_MUTEX);
        match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
            Ok(_) if GetLastError() == ERROR_ALREADY_EXISTS => false,
            Ok(_handle) => true, // intentionally leaked: held for the process lifetime
            Err(_) => true,
        }
    }
}

/// Tells the user why nothing launched instead of exiting silently.
fn notify_already_running() {
    unsafe {
        let text = to_wide("Glassbar is already running.");
        let caption = to_wide("Glassbar");
        MessageBoxW(
            HWND::default(),
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
}

fn main() {
    if !acquire_single_instance() {
        notify_already_running();
        return;
    }
    glassbar_lib::run()
}
