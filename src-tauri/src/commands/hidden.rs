//! Hidden background window commands (the tray-style hidden apps).

use crate::shell;

/// Background processes with no on-screen window (the tray-style hidden
/// apps). Enumeration walks every top-level window and extracts icons, so
/// it runs on a blocking worker.
#[tauri::command]
pub async fn hidden_windows() -> Result<Vec<shell::HiddenWindow>, String> {
    tauri::async_runtime::spawn_blocking(shell::hidden_windows)
        .await
        .map_err(|error| error.to_string())?
}

/// Reveals a hidden background window: restores it if minimized, shows it,
/// and brings it to the foreground. Runs on a blocking worker.
#[tauri::command]
pub async fn reveal_hidden_window(id: isize) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::reveal_hidden_window(id))
        .await
        .map_err(|error| error.to_string())?
}
