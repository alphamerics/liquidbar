//! Do not disturb commands: the Windows quiet-hours switch behind the
//! control center's Focus pill, plus the "sleep until 8:00 AM" focus window.

use crate::shell;
use serde::Serialize;

/// The Focus pill's state: whether Do not disturb is on and, when a sleep
/// window is armed, when it ends.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DndState {
    pub enabled: bool,
    pub expires_at_ms: Option<u64>,
}

#[tauri::command]
pub async fn dnd_state(app: tauri::AppHandle) -> DndState {
    let enabled = tauri::async_runtime::spawn_blocking(shell::dnd_enabled)
        .await
        .unwrap_or(false);
    DndState {
        enabled,
        expires_at_ms: shell::dnd_schedule(&app),
    }
}

/// Manual pill toggle. Any pending sleep window is cleared: the user took
/// over, so the automatic end-of-window no longer applies. Runs on a
/// blocking worker (registry access).
#[tauri::command]
pub async fn set_dnd(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = shell::set_dnd(enabled);
        shell::save_dnd_schedule(&app, None);
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Arms Do not disturb until the next 8:00 AM (the "Sleep" focus mode) and
/// schedules the automatic disable. Resolves with the window's end, epoch
/// millis. Runs on a blocking worker.
#[tauri::command]
pub async fn set_dnd_sleep(app: tauri::AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || shell::enable_dnd_sleep(&app))
        .await
        .map_err(|error| error.to_string())?
}
