//! Native system tray interaction — with no cursor automation at all.
//!
//! The custom taskbar is a PROXY for tray icon events. Every notification-area
//! icon an application registered via `Shell_NotifyIcon` is enumerated
//! directly from Explorer's tray toolbars (owner window + icon id + callback
//! message + image + tooltip); the callback metadata is stored once and
//! reused for every future interaction. When the user clicks an icon in the
//! custom tray UI, the exact notification messages Explorer would deliver to
//! the owning application are replayed to its callback window:
//!
//! * Left click: the tray activation message (`NIN_SELECT` for
//!   `NOTIFYICON_VERSION_4` registrations, the `WM_LBUTTONDOWN` /
//!   `WM_LBUTTONUP` pair for legacy ones).
//! * Right click: the context-menu event (`WM_CONTEXTMENU` for version 4,
//!   the `WM_RBUTTONDOWN` / `WM_RBUTTONUP` pair for legacy ones). The
//!   application then creates and displays its own native menu exactly as it
//!   would for a real right-click in Explorer — nothing is reconstructed or
//!   emulated here.
//!
//! No input is ever synthesized: no cursor movement, no `SendInput`, no
//! opening of the hidden-icon flyout, no screen coordinates or screen
//! scraping, and no dependency on Explorer's tray UI being visible. Explorer
//! keeps running normally — it is only ever read.

mod enumerate;
mod forward;

pub use enumerate::{enumerate_tray_icons, TrayIconInfo};
pub use forward::{forward_context_menu, forward_left_click, ForwardInfo};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};

/// Read-only diagnostic entry point for the `tray_probe` binary: enumerates
/// Explorer's tray service and dumps every stored registration. No input is
/// synthesized.
pub fn probe_report() -> String {
    let icons = enumerate_tray_icons();
    let mut report = format!("tray service enumerated: {} icons\n", icons.len());
    for icon in &icons {
        report.push_str(&format!(
            "  key={} hwnd={:x} uid={} callback={:#x} process={} tooltip={:?}\n",
            icon.key,
            icon.owner_hwnd,
            icon.uid,
            icon.callback_message,
            icon.process_name,
            icon.tooltip
        ));
    }
    report
}

/// Interactive diagnostic (`tray_probe --dispatch <key>`): forwards a real
/// context-menu event for one stored icon — the same messages the custom
/// tray UI's right-click delivers. No input is synthesized; the
/// application shows its own menu (dismiss it from the app itself).
pub fn probe_dispatch(key: &str) -> String {
    match forward::forward_context_menu(key) {
        Ok(info) => format!("{info:?}"),
        Err(error) => format!("FAILED: {error}"),
    }
}

/// The process id owning `hwnd` (`None` for a gone/invalid handle).
pub(crate) fn window_pid(hwnd: HWND) -> Option<u32> {
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

pub(crate) fn window_class(hwnd: HWND) -> Option<String> {
    let mut buffer = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}
