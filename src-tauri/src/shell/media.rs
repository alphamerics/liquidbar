//! Tracks the highest-priority active media session via Windows' media
//! transport controls (Spotify, browsers, and any app that registers with
//! SMTC), plus a live render-peak meter that drives the taskbar waveform.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::sync::Mutex;
use tauri::Emitter;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionMediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::{DataReader, InputStreamOptions};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSnapshot {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub app: Option<String>,
    pub is_playing: bool,
    /// The playback position when the snapshot was taken (seconds).
    pub position_secs: f64,
    /// The track's total length (seconds; 0 when the session reports none).
    pub duration_secs: f64,
}

static MEDIA: Mutex<MediaSnapshot> = Mutex::new(MediaSnapshot {
    title: None,
    artist: None,
    app: None,
    is_playing: false,
    position_secs: 0.0,
    duration_secs: 0.0,
});

/// The active session's playback timeline, sampled together with the media
/// poll so the position can be interpolated forward between polls.
#[derive(Clone, Copy)]
struct Timeline {
    position_secs: f64,
    duration_secs: f64,
    is_playing: bool,
    sampled_at: std::time::Instant,
}

static TIMELINE: Mutex<Option<Timeline>> = Mutex::new(None);

/// The playback timeline the UI's progress slider displays.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineInfo {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_playing: bool,
}

/// Seconds since the Windows epoch (1601-01-01 UTC) — the unit SMTC stamps
/// timeline update times in.
fn windows_epoch_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() + 11_644_473_600.0)
        .unwrap_or(0.0)
}

/// The live playback position: the poll samples the SMTC timeline only every
/// ~1.5s, so while the track is playing the position is advanced by the time
/// elapsed since that sample. Falling back to the raw sample when paused.
pub fn timeline() -> TimelineInfo {
    let timeline = TIMELINE
        .lock()
        .expect("media timeline lock poisoned");
    match *timeline {
        None => TimelineInfo::default(),
        Some(timeline) => {
            let position = if timeline.is_playing {
                timeline.position_secs + timeline.sampled_at.elapsed().as_secs_f64()
            } else {
                timeline.position_secs
            };
            TimelineInfo {
                position_secs: position,
                duration_secs: timeline.duration_secs,
                is_playing: timeline.is_playing,
            }
        }
    }
}

/// The source app of the session shown in the taskbar pill, so play/pause
/// targets the same session that is displayed (otherwise pausing one app and
/// resuming could hit a different, unrelated session).
static ACTIVE_SOURCE: Mutex<Option<String>> = Mutex::new(None);

/// The album art of the displayed track as a data URL, cached together with
/// the track key it belongs to so the ~1.5s poll does not re-decode the
/// thumbnail every cycle (the WinRT thumbnail round-trip is expensive).
static THUMBNAIL: Mutex<Option<(String, Option<String>)>> = Mutex::new(None);

/// The cached thumbnail data URL (None when the track has no art or the
/// fetch failed), pulled by the hover card over the audio gadget.
pub fn thumbnail() -> Option<String> {
    THUMBNAIL
        .lock()
        .expect("media thumbnail lock poisoned")
        .as_ref()
        .and_then(|(_, thumbnail)| thumbnail.clone())
}

/// Decodes the session's thumbnail (album art) into a data URL. Best-effort:
/// any failure yields None and the UI falls back to its placeholder.
fn fetch_thumbnail(
    properties: &GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<String> {
    let read = || -> Result<String, String> {        let reference = properties.Thumbnail().map_err(|error| error.to_string())?;
        let stream = reference
            .OpenReadAsync()
            .map_err(|error| error.to_string())?
            .get()
            .map_err(|error| error.to_string())?;
        let size = stream.Size().map_err(|error| error.to_string())?;
        if size == 0 || size > 8 * 1_048_576 {
            return Err("thumbnail missing or implausibly large".to_string());
        }
        let input = stream.GetInputStreamAt(0).map_err(|error| error.to_string())?;
        let reader = DataReader::CreateDataReader(&input).map_err(|error| error.to_string())?;
        reader
            .SetInputStreamOptions(InputStreamOptions::Partial)
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::with_capacity(size as usize);
        while (bytes.len() as u64) < size {
            let loaded = reader
                .LoadAsync((size - bytes.len() as u64) as u32)
                .map_err(|error| error.to_string())?
                .get()
                .map_err(|error| error.to_string())? as usize;
            if loaded == 0 {
                break;
            }
            let mut chunk = vec![0u8; loaded];
            reader
                .ReadBytes(&mut chunk)
                .map_err(|error| error.to_string())?;
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err("thumbnail stream was empty".to_string());
        }
        // Sniff the two common image formats first: the stream's declared
        // content type is often empty or wrong, and a mismatched type yields
        // a data URL the webview cannot render.
        let sniffed = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            Some("image/png")
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            Some("image/jpeg")
        } else if bytes.starts_with(b"GIF8") {
            Some("image/gif")
        } else if bytes.starts_with(&[b'B', b'M']) {
            Some("image/bmp")
        } else {
            None
        };
        let content_type = match sniffed {
            Some(content_type) => content_type.to_string(),
            None => {
                let declared = stream
                    .ContentType()
                    .ok()
                    .map(|content_type| content_type.to_string())
                    .unwrap_or_default();
                if declared.starts_with("image/") {
                    declared
                } else {
                    return Err(format!(
                        "unrecognized thumbnail format (declared {declared:?})"
                    ));
                }
            }
        };
        Ok(format!(
            "data:{content_type};base64,{}",
            STANDARD.encode(bytes)
        ))
    };
    match read() {
        Ok(data) => Some(data),
        Err(error) => {
            eprintln!("[media] thumbnail fetch failed: {error}");
            None
        }
    }
}

/// Cached SMTC manager. `RequestAsync` opens a new IPC channel to the Now
/// Playing Session Manager service (NPSMSvc); requesting it on every poll
/// pegs that service's CPU, so it is fetched once and reused until it fails.
static MANAGER: Mutex<Option<GlobalSystemMediaTransportControlsSessionManager>> = Mutex::new(None);

fn cached_manager() -> Option<GlobalSystemMediaTransportControlsSessionManager> {
    MANAGER
        .lock()
        .expect("media manager lock poisoned")
        .clone()
}

/// Evicts the cached manager so the next poll re-requests it (e.g. after
/// NPSMSvc restarts).
fn evict_manager() {
    *MANAGER.lock().expect("media manager lock poisoned") = None;
}

/// Starts the background thread that polls media sessions and streams the
/// render peak meter to the UI. COM must be initialized on the polling
/// thread; WinRT async calls are blocked on there.
pub fn start_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut has_logged_error = false;
        let mut meter: Option<IAudioMeterInformation> = None;
        let mut level: f32 = 0.0;
        let mut tick: f32 = 0.0;
        loop {
            match poll_media() {
                Ok(snapshot) => {
                    has_logged_error = false;
                    *MEDIA.lock().expect("media lock poisoned") = snapshot;
                }
                Err(error) => {
                    // The cached manager (or the service behind it) went
                    // away; drop it so the next poll re-requests.
                    evict_manager();
                    if !has_logged_error {
                        eprintln!("[media] unavailable: {error}");
                        has_logged_error = true;
                    }
                }
            }
            let playing = MEDIA.lock().expect("media lock poisoned").title.is_some();
            // ~20 level updates per second for 1.5 seconds, then the media
            // session poll runs again.
            for _ in 0..30 {
                if meter.is_none() {
                    meter = init_meter();
                }
                let peak = meter
                    .as_ref()
                    .and_then(|meter| unsafe { meter.GetPeakValue() }.ok())
                    .unwrap_or(0.0);
                let target = if playing { peak } else { 0.0 };
                // Fast fall, slow rise: keeps the waveform lively but smooth.
                level = target.max(level * 0.86);
                // Raw peaks are perceptually tiny (speech sits around 0.02);
                // compress and boost so the waveform visibly dances.
                let boost = level.clamp(0.0, 1.0).powf(0.45) * 1.35;
                let wave =
                    |phase: f32, speed: f32| (tick * speed + phase).sin().abs() * 0.55 + 0.45;
                let bars = [
                    (boost * wave(0.0, 0.9)).clamp(0.05, 1.0),
                    (boost * wave(2.1, 1.3)).clamp(0.05, 1.0),
                    (boost * wave(4.2, 0.75)).clamp(0.05, 1.0),
                    (boost * wave(1.4, 1.05)).clamp(0.05, 1.0),
                    (boost * wave(3.3, 0.6)).clamp(0.05, 1.0),
                ];
                tick += 1.0;
                let _ = app.emit("media-levels", bars);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    });
}

pub fn snapshot() -> MediaSnapshot {
    MEDIA.lock().expect("media lock poisoned").clone()
}

/// Runs a transport action (play/pause toggle, next, previous) on the active
/// session on a dedicated thread (WinRT needs COM; commands run on threads we
/// do not own). After the action it re-polls and publishes the fresh status
/// immediately so the buttons update without waiting for the next poll cycle.
pub fn transport(app: tauri::AppHandle, action: TransportAction) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let result = (|| -> Result<(), String> {
            let Some(session) = preferred_session()? else {
                return Ok(post_toggle_update(&app));
            };
            try_action(&session, action)?;
            Ok(post_toggle_update(&app))
        })();
        if let Err(error) = result {
            evict_manager();
            eprintln!("[media] transport failed: {error}");
        }
    });
}

fn post_toggle_update(app: &tauri::AppHandle) -> () {
    // Give the app a moment to report its new status, then publish it.
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Ok(snapshot) = poll_media() {
        *MEDIA.lock().expect("media lock poisoned") = snapshot.clone();
        let _ = app.emit("media-changed", &snapshot);
    }
}

/// The transport actions the UI can ask of the active session.
#[derive(Clone, Copy)]
pub enum TransportAction {
    TogglePlayPause,
    SkipNext,
    SkipPrevious,
    /// Jump to the given position (seconds).
    Seek(f64),
}

fn try_action(
    session: &GlobalSystemMediaTransportControlsSession,
    action: TransportAction,
) -> Result<(), String> {
    let operation = match action {
        TransportAction::TogglePlayPause => session.TryTogglePlayPauseAsync(),
        TransportAction::SkipNext => session.TrySkipNextAsync(),
        TransportAction::SkipPrevious => session.TrySkipPreviousAsync(),
        // SMTC takes the requested position in 100-nanosecond ticks.
        TransportAction::Seek(position_secs) => session
            .TryChangePlaybackPositionAsync((position_secs * 10_000_000.0) as i64),
    }
    .map_err(|error| error.to_string())?;
    let acted = operation.get().map_err(|error| error.to_string())?;
    if !acted {
        eprintln!("[media] the session declined the transport action");
    }
    Ok(())
}

/// Picks the session transport actions target: the one the pill is
/// displaying, then any playing session, then the first session at all.
fn preferred_session(
) -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
    let manager = session_manager()?;
    let sessions = manager.GetSessions().map_err(|error| error.to_string())?;
    let count = sessions.Size().map_err(|error| error.to_string())?;
    let preferred = ACTIVE_SOURCE
        .lock()
        .expect("media source lock poisoned")
        .clone();
    let mut fallback_playing: Option<GlobalSystemMediaTransportControlsSession> = None;
    let mut fallback_first: Option<GlobalSystemMediaTransportControlsSession> = None;
    for index in 0..count {
        let session = sessions.GetAt(index).map_err(|error| error.to_string())?;
        if let Some(preferred_source) = &preferred {
            if let Ok(source) = session.SourceAppUserModelId() {
                if clean_source(source.to_string()) == *preferred_source {
                    return Ok(Some(session));
                }
            }
        }
        let is_playing = session
            .GetPlaybackInfo()
            .ok()
            .and_then(|info| info.PlaybackStatus().ok())
            .is_some_and(|status| {
                status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
            });
        if is_playing && fallback_playing.is_none() {
            fallback_playing = Some(session.clone());
        }
        if fallback_first.is_none() {
            fallback_first = Some(session);
        }
    }
    Ok(fallback_playing.or(fallback_first))
}

fn session_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, String> {
    if let Some(manager) = cached_manager() {
        return Ok(manager);
    }
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|error| error.to_string())?
        .get()
        .map_err(|error| error.to_string())?;
    *MANAGER.lock().expect("media manager lock poisoned") = Some(manager.clone());
    Ok(manager)
}

/// The master render output's peak level (0..1), whatever app is playing.
fn init_meter() -> Option<IAudioMeterInformation> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .ok()?;
        device.Activate(CLSCTX_ALL, None).ok()
    }
}

fn poll_media() -> Result<MediaSnapshot, String> {
    let manager = session_manager()?;
    let sessions = manager.GetSessions().map_err(|error| error.to_string())?;
    let count = sessions.Size().map_err(|error| error.to_string())?;
    // Playing sessions outrank paused ones; the first playing session wins.
    // Only playback status is read per session (cheap); the async media
    // properties round-trip runs once, for the winning session.
    let mut best_index: Option<u32> = None;
    let mut best_playing = false;
    for index in 0..count {
        let session = sessions.GetAt(index).map_err(|error| error.to_string())?;
        let is_playing = session
            .GetPlaybackInfo()
            .ok()
            .and_then(|info| info.PlaybackStatus().ok())
            .is_some_and(|status| {
                status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
            });
        let replace = match best_index {
            None => true,
            Some(_) => candidate_beats(best_playing, is_playing),
        };
        if replace {
            best_index = Some(index);
            best_playing = is_playing;
        }
    }
    let Some(index) = best_index else {
        *TIMELINE.lock().expect("media timeline lock poisoned") = None;
        return Ok(MediaSnapshot::default());
    };
    let session = sessions.GetAt(index).map_err(|error| error.to_string())?;
    let properties = session
        .TryGetMediaPropertiesAsync()
        .map_err(|error| error.to_string())?
        .get()
        .map_err(|error| error.to_string())?;
    let title = properties.Title().map_err(|error| error.to_string())?;
    let snapshot = if title.is_empty() {
        *TIMELINE.lock().expect("media timeline lock poisoned") = None;
        MediaSnapshot::default()
    } else {
        let artist = properties.Artist().ok().map(|artist| artist.to_string());
        let app = session
            .SourceAppUserModelId()
            .ok()
            .map(|source| clean_source(source.to_string()));
        // Sample the session's timeline (position + track length) alongside
        // the poll; `timeline()` interpolates it forward while playing.
        let timeline = session.GetTimelineProperties().ok();
        let raw_position = timeline
            .as_ref()
            .and_then(|timeline| timeline.Position().ok())
            .map(|span| span.Duration as f64 / 10_000_000.0)
            .unwrap_or(0.0);
        let timeline_duration = timeline
            .as_ref()
            .and_then(|timeline| timeline.EndTime().ok())
            .map(|span| span.Duration as f64 / 10_000_000.0)
            .unwrap_or(0.0);
        // SMTC's Position is only accurate as of LastUpdatedTime, and
        // sources refresh that stamp at coarse intervals (Spotify ~5s), so
        // correct for the stamp's age — otherwise every poll re-samples a
        // stale base and the interpolated position jitters back and forth.
        let timeline_position = match timeline
            .as_ref()
            .and_then(|timeline| timeline.LastUpdatedTime().ok())
        {
            Some(stamp) if best_playing => {
                let age = windows_epoch_secs() - stamp.UniversalTime as f64 / 10_000_000.0;
                raw_position + age.clamp(0.0, 3600.0)
            }
            _ => raw_position,
        };
        *TIMELINE.lock().expect("media timeline lock poisoned") = Some(Timeline {
            position_secs: timeline_position,
            duration_secs: timeline_duration,
            is_playing: best_playing,
            sampled_at: std::time::Instant::now(),
        });
        MediaSnapshot {
            title: Some(title.to_string()),
            artist,
            app,
            is_playing: best_playing,
            position_secs: timeline_position,
            duration_secs: timeline_duration,
        }
    };
    if let Some(app) = &snapshot.app {
        *ACTIVE_SOURCE.lock().expect("media source lock poisoned") = Some(app.clone());
    }
    // Refresh the thumbnail cache only when the track actually changed: the
    // poll runs every ~1.5s but the WinRT art round-trip must not.
    if let Some(title) = &snapshot.title {
        let key = format!("{}|{}", snapshot.artist.as_deref().unwrap_or(""), title);
        let cached = THUMBNAIL.lock().expect("media thumbnail lock poisoned").clone();
        if !matches!(cached, Some((cached_key, _)) if cached_key == key) {
            let thumbnail = fetch_thumbnail(&properties);
            if thumbnail.is_none() {
                // Surfaced so a broken/missing art source is diagnosable in
                // the console instead of failing into a silent placeholder.
                eprintln!("[media] no thumbnail pulled for {key}");
            }
            *THUMBNAIL.lock().expect("media thumbnail lock poisoned") =
                Some((key, thumbnail));
        }
    }
    Ok(snapshot)
}

/// A playing session always beats a paused one; among equals the first wins.
fn candidate_beats(current_playing: bool, candidate_playing: bool) -> bool {
    candidate_playing && !current_playing
}

/// SMTC sources often end in ".exe"; keep a clean display/control id.
fn clean_source(raw: String) -> String {
    raw.strip_suffix(".exe").map(str::to_string).unwrap_or(raw)
}
