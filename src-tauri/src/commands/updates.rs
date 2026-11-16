//! Self-updating: check GitHub releases through tauri-plugin-updater and
//! hand the downloaded installer to the deploy installer (`/UPDATE` mode),
//! which swaps the files and relaunches the app.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize, Clone)]
pub struct UpdateStatus {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub current_version: String,
}

#[derive(Serialize, Clone)]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// One check against the update manifest. Shared by the settings button and
/// the silent startup check.
pub async fn run_check(app: &AppHandle) -> Result<UpdateStatus, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|error| format!("updater unavailable: {error}"))?;
    let update = updater
        .check()
        .await
        .map_err(|error| format!("update check failed: {error}"))?;
    let current_version = app.package_info().version.to_string();
    Ok(match update {
        Some(update) => UpdateStatus {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            current_version,
        },
        None => UpdateStatus {
            available: false,
            version: None,
            notes: None,
            current_version,
        },
    })
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    run_check(&app).await
}

/// Downloads the new installer (minisign-verified), then hands control to it.
/// On Windows the updater exits this process before installing; the deploy
/// installer relaunches the app afterwards (`/R /ARGS`).
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|error| format!("updater unavailable: {error}"))?;
    let update = updater
        .check()
        .await
        .map_err(|error| format!("update check failed: {error}"))?
        .ok_or("no update is available")?;

    let progress_app = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                static DOWNLOADED: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let downloaded = DOWNLOADED.fetch_add(chunk as u64, std::sync::atomic::Ordering::Relaxed)
                    + chunk as u64;
                let _ = progress_app.emit(
                    "update://progress",
                    UpdateProgress {
                        downloaded,
                        total,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|error| format!("install failed: {error}"))?;
    let _ = app.emit("update://downloaded", ());
    Ok(())
}
