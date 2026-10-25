//! Notification-area commands: enumeration and interaction forwarding to the
//! owning applications.

use crate::shell;

/// Enumerates the notification-area icons Explorer holds (see shell::tray).
/// The cold scan walks Explorer's committed memory and probes the shell per
/// record — seconds of blocking work — so it runs on a blocking worker: on
/// the main thread it would freeze the whole taskbar (no glass resizes, no
/// clicks) for the duration. Warm calls hit the shell's cache (window
/// environment unchanged, enumeration recent) and return almost instantly,
/// which is what the control center's refresh interval usually gets.
#[tauri::command]
pub async fn tray_icons() -> Result<Vec<shell::TrayIconInfo>, String> {
    tauri::async_runtime::spawn_blocking(shell::enumerate_tray_icons)
        .await
        .map_err(|error| error.to_string())
}

/// Left click on a custom tray icon: forwards the tray activation message to
/// the owning application. Delivery blocks (reaction probing), so this runs
/// on a blocking worker.
#[tauri::command]
pub async fn tray_icon_activate(key: String) -> Result<shell::ForwardInfo, String> {
    tauri::async_runtime::spawn_blocking(move || shell::forward_left_click(&key))
        .await
        .map_err(|error| error.to_string())?
}

/// Right click on a custom tray icon: sends the tray context-menu event so
/// the application shows its own native menu. Runs on a blocking worker.
#[tauri::command]
pub async fn tray_icon_context_menu(key: String) -> Result<shell::ForwardInfo, String> {
    tauri::async_runtime::spawn_blocking(move || shell::forward_context_menu(&key))
        .await
        .map_err(|error| error.to_string())?
}
