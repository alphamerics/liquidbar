//! Master volume and per-app ("program mixing") volume via Core Audio.
//!
//! The mixer enumerates the default render endpoint's audio sessions; each
//! session belongs to a process, so the UI can show one volume slider per
//! running audio app. All calls run COM on the calling thread — commands
//! invoke these on a blocking worker thread.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use windows::core::Interface;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

/// One app's audio session in the mixer.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApp {
    pub pid: u32,
    pub name: String,
    /// The app's real icon as a PNG data URL (from its executable).
    pub icon: Option<String>,
    pub volume: f64,
    pub muted: bool,
}

/// Cached `(display name, icon)` per executable path: icon extraction is
/// GDI + PNG encode + base64 work, and the mixer is polled every few
/// seconds, so nothing may re-extract per poll.
static APP_VISUALS: LazyLock<Mutex<HashMap<String, (String, Option<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The whole mixer state: master output volume plus the per-app sessions.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSnapshot {
    pub master: f64,
    pub master_muted: bool,
    pub apps: Vec<AudioApp>,
}

/// Initializes COM on the calling thread (safe to call repeatedly).
fn ensure_com() {
    unsafe {
        // RPC_E_CHANGED_MODE just means COM is already initialized on this
        // thread with a different apartment model; either way it is usable.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn endpoint_volume() -> Result<IAudioEndpointVolume, String> {
    ensure_com();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| error.to_string())?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|error| error.to_string())?;
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| error.to_string())
    }
}

fn session_manager() -> Result<IAudioSessionManager2, String> {
    ensure_com();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| error.to_string())?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|error| error.to_string())?;
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| error.to_string())
    }
}

pub fn master_snapshot() -> Result<(f64, bool), String> {
    let volume = endpoint_volume()?;
    unsafe {
        let level = volume
            .GetMasterVolumeLevelScalar()
            .map_err(|error| error.to_string())?;
        let muted = volume.GetMute().map_err(|error| error.to_string())?;
        Ok((level as f64, muted.as_bool()))
    }
}

pub fn set_master_volume(value: f64) -> Result<(), String> {
    let volume = endpoint_volume()?;
    unsafe {
        volume
            .SetMasterVolumeLevelScalar(value.clamp(0.0, 1.0) as f32, std::ptr::null())
            .map_err(|error| error.to_string())
    }
}

pub fn set_master_muted(muted: bool) -> Result<(), String> {
    let volume = endpoint_volume()?;
    unsafe {
        volume
            .SetMute(muted, std::ptr::null())
            .map_err(|error| error.to_string())
    }
}

/// Enumerates the render endpoint's audio sessions (the app mixer).
///
/// Every session that belongs to a real process is listed — including
/// inactive ones — matching the Windows Volume Mixer, which keeps showing an
/// app after its audio pauses. A paused app's session drops into the
/// `Inactive` state but is still alive and adjustable; only system sounds
/// (pid 0) and truly expired sessions are left out. Multiple sessions of the
/// same app merge into one row (the active one wins for the reported state).
pub fn app_mix() -> Result<Vec<AudioApp>, String> {
    let manager = session_manager()?;
    unsafe {
        let enumerator: IAudioSessionEnumerator = manager
            .GetSessionEnumerator()
            .map_err(|error| error.to_string())?;
        let count = enumerator.GetCount().map_err(|error| error.to_string())?;
        let mut apps: Vec<AudioApp> = Vec::new();
        for index in 0..count {
            let Ok(control) = enumerator.GetSession(index) else {
                continue;
            };
            let Ok(session) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let Ok(pid) = session.GetProcessId() else {
                continue;
            };
            if pid == 0 {
                // System sounds have no app behind them.
                continue;
            }
            let Ok(simple) = control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let state = session.GetState().unwrap_or_default();
            let volume = simple
                .GetMasterVolume()
                .map(|value| value as f64)
                .unwrap_or(1.0);
            let muted = simple.GetMute().map(|m| m.as_bool()).unwrap_or(false);
            let (name, icon) = app_visual(pid);
            let app = AudioApp {
                pid,
                name,
                icon,
                volume,
                muted,
            };
            match apps.iter_mut().find(|existing| existing.pid == pid) {
                Some(existing) => {
                    // The active session represents the app's live state.
                    if state.0 == 1 {
                        *existing = app;
                    }
                }
                None => apps.push(app),
            }
        }
        Ok(apps)
    }
}

/// Sets the volume of EVERY audio session belonging to the pid — apps can
/// hold several sessions (one per output stream), and the mixer row controls
/// the app as a whole.
pub fn set_app_volume(pid: u32, value: f64) -> Result<(), String> {
    let manager = session_manager()?;
    unsafe {
        let enumerator: IAudioSessionEnumerator = manager
            .GetSessionEnumerator()
            .map_err(|error| error.to_string())?;
        let count = enumerator.GetCount().map_err(|error| error.to_string())?;
        let mut touched = false;
        let mut last_error = None;
        for index in 0..count {
            let Ok(control) = enumerator.GetSession(index) else {
                continue;
            };
            let Ok(session) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if session.GetProcessId() != Ok(pid) {
                continue;
            }
            let Ok(simple) = control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            touched = true;
            if let Err(error) =
                simple.SetMasterVolume(value.clamp(0.0, 1.0) as f32, std::ptr::null())
            {
                last_error = Some(error.to_string());
            }
        }
        match (touched, last_error) {
            (false, _) => Err(format!("no audio session for pid {pid}")),
            (true, Some(error)) => Err(error),
            (true, None) => Ok(()),
        }
    }
}

/// The mixer row's identity for a process id: the executable's file stem
/// (no `.exe` extension) and its real icon as a PNG data URL. Both are
/// cached per executable path — only the first poll for an app pays the
/// icon extraction.
fn app_visual(pid: u32) -> (String, Option<String>) {
    let Some(wide) = crate::shell::icons::executable_path(pid) else {
        return (format!("pid {pid}"), None);
    };
    let path = String::from_utf16_lossy(&wide[..wide.len().saturating_sub(1)]);
    let mut visuals = APP_VISUALS.lock().expect("app visuals lock poisoned");
    visuals
        .entry(path.clone())
        .or_insert_with(|| {
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("pid {pid}"));
            let icon = crate::shell::icons::icon_for_path(&path);
            (name, icon)
        })
        .clone()
}
