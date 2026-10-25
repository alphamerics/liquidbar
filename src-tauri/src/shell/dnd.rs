//! Windows "Do not disturb": the same quiet-hours switch Windows 11's own
//! quick settings flips. The state lives in the toast platform's per-user
//! registry key (`ToastEnabled` under `...\PushNotifications`; 0 = DND on),
//! which needs no admin rights. The "sleep" mode arms a window that ends at
//! 8:00 AM: a watcher thread turns DND off when it elapses, and the window
//! is persisted to disk so a shutdown/reboot can't leave DND stuck on past
//! its end — the boot check reconciles it.

use chrono::{Duration as ChronoDuration, Local, NaiveTime};
use serde::{Deserialize, Serialize};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const DND_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications";
const DND_VALUE: &str = "ToastEnabled";
const DND_FILE: &str = "dnd.json";

/// Whether Do not disturb is on. A missing key/value means notifications are
/// allowed (DND off), the Windows default.
pub fn dnd_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey_with_flags(DND_SUBKEY, KEY_READ) else {
        return false;
    };
    key.get_value::<u32, _>(DND_VALUE)
        .map(|value| value == 0)
        .unwrap_or(false)
}

pub fn set_dnd(enabled: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(DND_SUBKEY)
        .map_err(|error| error.to_string())?;
    key.set_value(DND_VALUE, &if enabled { 0u32 } else { 1u32 })
        .map_err(|error| error.to_string())?;
    broadcast_setting_change();
    Ok(())
}

/// Nudges the shell the way the system does on a settings change, so apps
/// notice the new notification mode without a restart.
fn broadcast_setting_change() {
    unsafe {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
}

/// The "sleep" focus window: when Do not disturb should turn itself off.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DndSchedule {
    expires_at_ms: u64,
}

/// The armed window's end (epoch millis), when one is pending.
pub fn dnd_schedule(app: &tauri::AppHandle) -> Option<u64> {
    let path = super::data_file(app, DND_FILE)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: DndSchedule = serde_json::from_str(&raw).ok()?;
    Some(parsed.expires_at_ms)
}

/// Arms (or with `None` clears) the persisted sleep window.
pub fn save_dnd_schedule(app: &tauri::AppHandle, expires_at_ms: Option<u64>) {
    let Some(path) = super::data_file(app, DND_FILE) else {
        return;
    };
    match expires_at_ms {
        Some(ms) => {
            let raw = serde_json::to_string(&DndSchedule { expires_at_ms: ms }).unwrap_or_default();
            if let Err(error) = std::fs::write(&path, raw) {
                eprintln!("Could not save {}: {error}", path.display());
            }
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Epoch millis of the next 8:00 AM local time: today's if it is still in
/// the future, otherwise tomorrow's.
fn next_8am_ms() -> u64 {
    let now = Local::now();
    let eight = NaiveTime::from_hms_opt(8, 0, 0).expect("8:00 AM is a valid time");
    let target = if now.time() >= eight {
        now.date_naive().and_time(eight) + ChronoDuration::days(1)
    } else {
        now.date_naive().and_time(eight)
    };
    match target.and_local_timezone(now.timezone()) {
        chrono::LocalResult::Single(dt) => dt,
        // A repeated 8:00 AM (DST fall-back) reads as the earlier one.
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        // A nonexistent 8:00 AM (rare DST gaps) degrades to UTC; the drift
        // is at most an hour and only in exotic zones.
        chrono::LocalResult::None => target.and_utc().into(),
    }
    .timestamp_millis() as u64
}

/// Arms the watcher thread that turns Do not disturb off when the window
/// ends. It re-reads the schedule at wake-up, so a re-scheduled (or cleared)
/// window supersedes an older thread instead of fighting it.
fn spawn_schedule_watcher(app: tauri::AppHandle, expires_at_ms: u64) {
    let spawned = std::thread::Builder::new()
        .name("dnd-schedule".into())
        .spawn(move || {
            let mut remaining = expires_at_ms.saturating_sub(now_ms());
            while remaining > 0 {
                let step = remaining.min(60_000);
                std::thread::sleep(std::time::Duration::from_millis(step));
                remaining = expires_at_ms.saturating_sub(now_ms());
            }
            if dnd_schedule(&app) == Some(expires_at_ms) {
                let _ = set_dnd(false);
                save_dnd_schedule(&app, None);
            }
        });
    if let Err(error) = spawned {
        eprintln!("Could not start the do-not-disturb watcher: {error}");
    }
}

/// Do not disturb on, until the next 8:00 AM. Resolves with the window's
/// end (epoch millis) so the UI can label it.
pub fn enable_dnd_sleep(app: &tauri::AppHandle) -> Result<u64, String> {
    let expires = next_8am_ms();
    set_dnd(true)?;
    save_dnd_schedule(app, Some(expires));
    spawn_schedule_watcher(app.clone(), expires);
    Ok(expires)
}

/// Boot-time reconciliation for the sleep window: if the pc was shut down
/// while a window was armed and restarted after its end, the watcher never
/// fired — turn Do not disturb off now and clear the stale window. An
/// unfinished window re-arms its watcher instead.
pub fn boot_check(app: &tauri::AppHandle) {
    let Some(expires) = dnd_schedule(app) else {
        return;
    };
    if now_ms() >= expires {
        let _ = set_dnd(false);
        save_dnd_schedule(app, None);
        eprintln!("[boot] do-not-disturb sleep window expired while the pc was off; DND off");
    } else {
        spawn_schedule_watcher(app.clone(), expires);
    }
}
