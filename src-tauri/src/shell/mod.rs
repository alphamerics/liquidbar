mod airplane;
mod appbar;
mod audio;
mod battery;
mod bluetooth;
mod dnd;
mod icons;
mod media;
mod monitors;
mod night_light;
pub mod preview;
mod radios;

mod hidden;
pub mod keyboard;
pub use keyboard::KeyboardLayout;
mod taskbar_state;
mod tray;
mod wifi;
mod windows;

pub use airplane::set_airplane_mode;
pub use dnd::{
    boot_check as dnd_boot_check, dnd_enabled, dnd_schedule, enable_dnd_sleep, save_dnd_schedule,
    set_dnd,
};
pub use audio::{
    app_mix as audio_app_mix, master_snapshot as audio_master,
    set_app_volume as set_audio_app_volume, set_master_muted as set_audio_muted,
    set_master_volume as set_audio_master_volume, AudioSnapshot,
};
pub use battery::{battery_snapshot, BatterySnapshot};
pub use bluetooth::{
    devices as bluetooth_devices, pair as bluetooth_pair, unpair as bluetooth_unpair,
    BluetoothDevice,
};
pub use hidden::{hidden_windows, reveal as reveal_hidden_window, HiddenWindow};
pub use media::{
    snapshot as media_snapshot, start_monitor as start_media_monitor,
    thumbnail as media_thumbnail, timeline as media_timeline, transport as media_transport,
    MediaSnapshot, TimelineInfo, TransportAction,
};
pub use night_light::{reset_night_light, set_night_light};
pub use radios::{bluetooth_enabled, set_bluetooth_enabled, set_wifi_enabled, wifi_enabled};
pub use taskbar_state::{restore_taskbar, set_taskbar_hidden, start_taskbar_monitor};
pub use tray::{
    enumerate_tray_icons, forward_context_menu, forward_left_click,
    probe_dispatch as tray_probe_dispatch, probe_report as tray_probe_report, ForwardInfo,
    TrayIconInfo,
};
pub use wifi::{
    connect as wifi_connect, disconnect as wifi_disconnect, networks as wifi_networks, WifiNetwork,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarApp {
    id: isize,
    title: String,
    process_name: String,
    icon: Option<String>,
    is_active: bool,
    is_minimized: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    apps: Vec<TaskbarApp>,
    monitor_count: u32,
    pinned: Vec<PinnedApp>,
}

/// A pinned app: rendered on the taskbar even when the app isn't running
/// (clicking it launches the stored executable).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PinnedApp {
    pub process_name: String,
    pub title: String,
    pub icon: Option<String>,
    pub executable: Option<String>,
}

#[derive(Default)]
pub struct ShellState {
    app_order: Mutex<Vec<isize>>,
    pub pinned: Mutex<Vec<PinnedApp>>,
    /// Icon data URLs extracted from pinned executables, cached by path so
    /// the snapshot loop doesn't re-encode icons every 2 seconds.
    icon_cache: Mutex<HashMap<String, Option<String>>>,
}

pub fn snapshot(app: &tauri::AppHandle, state: &ShellState) -> ShellSnapshot {
    let mut apps = windows::enumerate();
    let mut order = state.app_order.lock().expect("app order lock poisoned");
    order.retain(|id| apps.iter().any(|app| app.id == *id));
    for app in &apps {
        if !order.contains(&app.id) {
            order.push(app.id);
        }
    }
    apps.sort_by_key(|app| {
        order
            .iter()
            .position(|id| *id == app.id)
            .expect("enumerated app must have an order")
    });

    // Enrich pinned entries in place: running apps backfill their title, icon
    // and executable path (self-heals legacy bare-name pins), and closed apps
    // fetch their icon from the stored executable so their buttons render
    // properly while closed.
    let mut pinned = state.pinned.lock().expect("pinned lock poisoned");
    let mut dirty = false;
    for entry in pinned.iter_mut() {
        let running = apps
            .iter()
            .find(|candidate| candidate.process_name == entry.process_name);
        match running {
            Some(app) => {
                if entry.executable.is_none() {
                    entry.executable = windows::executable_path_for_window(app.id);
                    dirty = true;
                }
                if entry.icon.is_none() {
                    entry.icon = app.icon.clone();
                    dirty = true;
                }
                if entry.title.is_empty() {
                    entry.title = app.title.clone();
                    dirty = true;
                }
            }
            None => {
                if let Some(path) = entry.executable.clone() {
                    let mut cache = state.icon_cache.lock().expect("icon cache lock poisoned");
                    let icon = cache
                        .entry(path.clone())
                        .or_insert_with(|| icons::icon_for_path(&path));
                    if entry.icon.is_none() {
                        entry.icon = icon.clone();
                        dirty = true;
                    }
                }
            }
        }
    }
    drop(pinned);
    if dirty {
        shell_pinned_dirty(app, state);
    }

    ShellSnapshot {
        apps,
        monitor_count: monitors::count(),
        pinned: state.pinned.lock().expect("pinned lock poisoned").clone(),
    }
}

/// Persists enriched pinned entries without touching ordering.
fn shell_pinned_dirty(app: &tauri::AppHandle, state: &ShellState) {
    let pinned = state.pinned.lock().expect("pinned lock poisoned").clone();
    save_pinned(app, &pinned);
}

pub fn activate(window_id: isize, minimize_toggle: bool) -> Result<(), String> {
    windows::activate(window_id, minimize_toggle)
}

pub fn close_window(window_id: isize) -> Result<(), String> {
    windows::close(window_id)
}

/// Builds a pinned entry from a running window (title, process, icon and the
/// executable path, so the app can be launched while closed).
pub fn pinned_entry_for_window(window_id: isize) -> Option<PinnedApp> {
    let apps = windows::enumerate();
    let app = apps.into_iter().find(|app| app.id == window_id)?;
    let executable = windows::executable_path_for_window(window_id);
    Some(PinnedApp {
        process_name: app.process_name,
        title: app.title,
        icon: app.icon,
        executable,
    })
}

/// Launches a pinned app's stored executable.
pub fn launch_executable(path: &str) -> Result<(), String> {
    windows::launch_executable(path)
}

/// Loads the pinned apps saved in the app data dir as pinned.json. Older
/// files stored bare process-name strings; those are upgraded in place.
pub fn load_pinned(app: &tauri::AppHandle) -> Vec<PinnedApp> {
    let Some(path) = data_file(app, "pinned.json") else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum PinnedEntry {
        Full(PinnedApp),
        Legacy(String),
    }
    let entries: Vec<PinnedEntry> = serde_json::from_str(&raw).unwrap_or_else(|error| {
        eprintln!("Could not parse {}: {error}", path.display());
        Vec::new()
    });
    entries
        .into_iter()
        .map(|entry| match entry {
            PinnedEntry::Full(app) => app,
            PinnedEntry::Legacy(name) => PinnedApp {
                process_name: name.clone(),
                title: name,
                icon: None,
                executable: None,
            },
        })
        .collect()
}

/// Saves the pinned apps to the app data dir as pinned.json.
pub fn save_pinned(app: &tauri::AppHandle, pinned: &[PinnedApp]) {
    let Some(path) = data_file(app, "pinned.json") else {
        return;
    };
    let raw = serde_json::to_string_pretty(pinned).unwrap_or_else(|_| "[]".to_string());
    if let Err(error) = std::fs::write(&path, raw) {
        eprintln!("Could not save {}: {error}", path.display());
    }
}

/// Loads the user settings saved in the app data dir as settings.json,
/// falling back to the defaults for anything missing or unparsable. A saved
/// layout that still matches the pre-battery default at format version 0 is
/// upgraded once (and persisted) so the newer widgets appear — done here, at
/// load, so the upgrade can never resurrect widgets the user later removed:
/// every explicit layout write bumps the layout version, which suppresses
/// the migration.
pub fn load_settings(app: &tauri::AppHandle) -> crate::GlassbarSettings {
    // The shared material lives in the broker-owned global file; seed it
    // from the legacy local keys if this is an upgrade, then overlay it
    // onto whatever settings.json holds (the material fields are
    // serde-skipped there, so they always come from the global file).
    if let Err(error) = liquid_glass::broker::seed_material_if_missing() {
        eprintln!("Could not seed the glass material: {error}");
    }
    let mut settings = load_persisted_settings(app);
    let material = liquid_glass::broker::read_material();
    crate::settings::apply_material(&mut settings, &material);
    settings
}

/// The settings.json round-trip only: parse (with defaults) and the legacy
/// control-center layout migration. Material fields are overlaid by the
/// caller (`load_settings`).
fn load_persisted_settings(app: &tauri::AppHandle) -> crate::GlassbarSettings {
    let Some(path) = data_file(app, "settings.json") else {
        return crate::GlassbarSettings::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return crate::GlassbarSettings::default();
    };
    let mut settings: crate::GlassbarSettings =
        match serde_json::from_str(&raw) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("Could not parse {}: {error}", path.display());
                return crate::GlassbarSettings::default();
            }
        };
    if crate::settings::upgrade_legacy_cc_layout(&mut settings) {
        save_settings(app, &settings);
    }
    settings
}

/// Saves the user settings to the app data dir as settings.json. The
/// glass material fields are NOT persisted here: they are written to the
/// broker-owned global material file (`%LOCALAPPDATA%\LiquidGlass\\
/// material.json`), which every liquid-glass app shares — the settings
/// window edits the global material.
pub fn save_settings(app: &tauri::AppHandle, settings: &crate::GlassbarSettings) {
    let Some(path) = data_file(app, "settings.json") else {
        return;
    };
    let Ok(raw) = serde_json::to_string_pretty(settings) else {
        return;
    };
    if let Err(error) = std::fs::write(&path, raw) {
        eprintln!("Could not save {}: {error}", path.display());
    }
    let material = crate::settings::material_from(settings);
    if let Err(error) = liquid_glass::broker::write_material(&material) {
        eprintln!("Could not save the glass material: {error}");
    }
}

/// Syncs the Windows per-user Run key with the "open on startup" setting:
/// an enabled setting writes the app's own executable path (quoted, since
/// paths can contain spaces); a disabled one deletes the value. Best-effort:
/// a registry failure never blocks a settings save, it only logs.
pub fn set_launch_on_startup(enabled: bool) {
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "Glassbar";
    let result = (|| -> Result<(), String> {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(RUN_KEY)
            .map_err(|error| error.to_string())?
            .0;
        if enabled {
            let exe = std::env::current_exe()
                .map_err(|error| error.to_string())?
                .display()
                .to_string();
            key.set_value(VALUE_NAME, &format!("\"{exe}\""))
                .map_err(|error| error.to_string())?;
        } else {
            // A missing value means "not launched at startup" already: not
            // an error.
            let _ = key.delete_value(VALUE_NAME);
        }
        Ok(())
    })();
    match result {
        Ok(()) => eprintln!("[startup] run key updated (enabled={enabled})"),
        Err(error) => eprintln!("[startup] could not update the run key: {error}"),
    }
}

fn data_file(app: &tauri::AppHandle, name: &str) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(name))
}

pub use appbar::{overlay_bottom_y, OVERLAY_HEIGHT_PX, TASKBAR_HEIGHT_PX};
pub use windows::cursor_visible;
pub use windows::foreground_cover;
pub use windows::start_foreground_tracker;

pub fn register_appbar(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    appbar::register(hwnd, desired_width_px)
}

pub fn reposition_appbar(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    appbar::reposition(hwnd, desired_width_px)
}

/// Places the overlay without an AppBar reservation (floating pill).
pub fn reposition_unregistered(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    appbar::reposition_unregistered(hwnd, desired_width_px)
}

pub fn taskbar_width_px(desired_width_px: i32) -> i32 {
    appbar::taskbar_width_px(desired_width_px)
}

pub fn unregister_appbar(hwnd: isize) {
    appbar::unregister(hwnd);
}

pub fn install_activation_subclass(hwnd: isize) -> Result<(), String> {
    appbar::install_activation_subclass(hwnd)
}
