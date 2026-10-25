//! Media & audio commands: the media transport snapshot/play toggle and the
//! per-app audio mixer.

use crate::shell;

#[tauri::command]
pub fn media_snapshot() -> shell::MediaSnapshot {
    shell::media_snapshot()
}

#[tauri::command]
pub fn toggle_media_play(app: tauri::AppHandle) {
    shell::media_transport(app, shell::TransportAction::TogglePlayPause);
}

#[tauri::command]
pub fn media_next(app: tauri::AppHandle) {
    shell::media_transport(app, shell::TransportAction::SkipNext);
}

#[tauri::command]
pub fn media_previous(app: tauri::AppHandle) {
    shell::media_transport(app, shell::TransportAction::SkipPrevious);
}

/// The live playback timeline (position interpolated between polls) that the
/// media card's progress slider displays.
#[tauri::command]
pub fn media_timeline() -> shell::TimelineInfo {
    shell::media_timeline()
}

#[tauri::command]
pub fn media_seek(app: tauri::AppHandle, position_secs: f64) {
    shell::media_transport(app, shell::TransportAction::Seek(position_secs));
}

/// The displayed track's album art as a data URL (the hover card over the
/// audio gadget fetches it lazily, only while it is open).
#[tauri::command]
pub fn media_thumbnail() -> Option<String> {
    shell::media_thumbnail()
}

/// The audio mixer state (master + per-app volumes). Runs on a blocking
/// worker thread: COM session enumeration must not stall the main thread.
#[tauri::command]
pub async fn audio_snapshot() -> Result<shell::AudioSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| -> Result<shell::AudioSnapshot, String> {
        let (master, muted) = shell::audio_master()?;
        let apps = shell::audio_app_mix().unwrap_or_default();
        Ok(shell::AudioSnapshot {
            master,
            master_muted: muted,
            apps,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_audio_master(volume: f64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_audio_master_volume(volume))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_audio_muted(muted: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_audio_muted(muted))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_audio_app(pid: u32, volume: f64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_audio_app_volume(pid, volume))
        .await
        .map_err(|error| error.to_string())?
}
