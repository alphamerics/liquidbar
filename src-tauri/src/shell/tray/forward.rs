//! Callback forwarding: replays, to the owning application, the exact
//! notification messages Explorer delivers when the user interacts with a
//! real tray icon.
//!
//! Which message set is version-correct depends on the registration's
//! notification-icon version (`NOTIFYICONDATA.uVersion`):
//!
//! * Version 4 (`NOTIFYICON_VERSION_4`): one message per interaction —
//!   `NIN_SELECT` for a left click, `WM_CONTEXTMENU` for a right click —
//!   with the anchor coordinates packed into `wParam` (X low word, Y high
//!   word) and the event in `LOWORD(lParam)`, icon id in `HIWORD(lParam)`.
//! * Versions 0/3 (legacy): one message per mouse transition — the
//!   `WM_LBUTTONDOWN`/`WM_LBUTTONUP` pair for a left click, the
//!   `WM_RBUTTONDOWN`/`WM_RBUTTONUP` pair for a right click — with the icon
//!   id in `wParam` and the mouse message in `lParam`.
//!
//! The version is discovered the first time an icon is used and cached in
//! [`ICON_VERSIONS`] so later interactions deliver a single version-correct
//! event. While it is unknown, the version-4 event is tried first and the
//! legacy pair second: each is a no-op for an application registered under
//! the other version, so the pair of attempts replicates every Explorer
//! behavior without ever synthesizing input.
//!
//! The anchor for version-4 messages is the user's real cursor position —
//! read-only. The user physically clicked the icon in the custom taskbar, so
//! the cursor already sits where Explorer would have placed the menu; there
//! is no cursor movement, only `GetCursorPos`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetGUIThreadInfo, GetSystemMetrics, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, PostMessageW, SetForegroundWindow, GUITHREADINFO, SM_CXSCREEN,
    SM_CYSCREEN, WM_CONTEXTMENU, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

use super::enumerate::{enumerate_tray_icons, stored_icon, TrayIconInfo};
use super::window_pid;

/// Notification-icon versions (`NOTIFYICONDATA.uVersion`).
const NOTIFYICON_VERSION_4: u32 = 4;
const NOTIFYICON_VERSION_LEGACY: u32 = 0;

/// GUITHREADINFO flag bits: the thread is inside a menu loop (winuser.h).
const GUI_INMENUMODE: u32 = 0x0004;
const GUI_SYSTEMMENUMODE: u32 = 0x0008;
const GUI_POPUPMENUMODE: u32 = 0x0010;

/// How long to wait for the application's visible reaction (its own menu or
/// window) before falling back to the other message set. Webview-backed
/// popup windows (Steam, Electron apps) can take a moment to appear.
const REACTION_TIMEOUT: Duration = Duration::from_millis(1500);

/// Pause between the down and up message of a legacy click pair — the
/// minimum gap Explorer's real transitions leave between the two.
const LEGACY_PAIR_GAP: Duration = Duration::from_millis(50);

/// The notification-icon version discovered per registration key, once an
/// interaction reveals it. Stored once and reused for future interactions.
static ICON_VERSIONS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// How a forwarded interaction was delivered — surfaced for diagnostics.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    /// The window that received the callback messages.
    pub owner_hwnd: isize,
    /// The registration's icon id.
    pub uid: u32,
    /// The callback message the events were posted with.
    pub callback_message: u32,
    /// The notification-icon version the events were built for, when known.
    pub version: Option<u32>,
    /// Whether the application visibly reacted (opened its menu/window).
    pub reacted: bool,
    /// The delivered events, e.g. "WM_CONTEXTMENU" or
    /// "WM_RBUTTONDOWN+WM_RBUTTONUP".
    pub events: &'static str,
}

#[derive(Clone, Copy)]
enum ClickKind {
    Left,
    Right,
}

/// Left click on a custom tray icon: forwards the equivalent tray
/// activation message to the owning application, replicating Explorer's
/// behavior as closely as possible.
pub fn forward_left_click(key: &str) -> Result<ForwardInfo, String> {
    deliver(key, ClickKind::Left)
}

/// Right click on a custom tray icon: sends the tray context-menu event to
/// the application's registered callback window so the application creates
/// and displays its own native menu, exactly as it would for a real
/// right-click in Explorer.
pub fn forward_context_menu(key: &str) -> Result<ForwardInfo, String> {
    deliver(key, ClickKind::Right)
}

fn deliver(key: &str, kind: ClickKind) -> Result<ForwardInfo, String> {
    let icon = resolve_icon(key)?;
    let owner = HWND(icon.owner_hwnd as *mut _);
    let thread_id = unsafe { GetWindowThreadProcessId(owner, None) };
    let windows_before = visible_windows_of(window_pid(owner));

    // Hand over activation: the application must be allowed to bring itself
    // to the foreground inside its handler, or its TrackPopupMenu would
    // never close on outside clicks (the classic broken tray-menu symptom).
    unsafe {
        let _ = SetForegroundWindow(owner);
    }
    std::thread::sleep(Duration::from_millis(60));

    let anchor = cursor_anchor();
    let version = cached_version(key);

    let (version, events, reacted) = match version {
        Some(version) => {
            let events = if version == NOTIFYICON_VERSION_4 {
                post_version_4(&icon, kind, anchor)
            } else {
                post_legacy(&icon, kind)
            };
            let reacted = wait_for_reaction(thread_id, window_pid(owner), &windows_before);
            (Some(version), events, reacted)
        }
        None => {
            // Unknown version: try the version-4 event first, then the legacy
            // pair. Each is inert for registrations of the other version, so
            // the combination replicates both Explorer behaviors.
            let v4_events = post_version_4(&icon, kind, anchor);
            if wait_for_reaction(thread_id, window_pid(owner), &windows_before) {
                cache_version(key, NOTIFYICON_VERSION_4);
                (Some(NOTIFYICON_VERSION_4), v4_events, true)
            } else {
                let legacy_events = post_legacy(&icon, kind);
                let reacted = wait_for_reaction(thread_id, window_pid(owner), &windows_before);
                if reacted {
                    cache_version(key, NOTIFYICON_VERSION_LEGACY);
                }
                (
                    if reacted {
                        Some(NOTIFYICON_VERSION_LEGACY)
                    } else {
                        None
                    },
                    if reacted {
                        legacy_events
                    } else {
                        // Nothing reacted; both message sets were delivered.
                        concat_events(v4_events, legacy_events)
                    },
                    false,
                )
            }
        }
    };

    Ok(ForwardInfo {
        owner_hwnd: icon.owner_hwnd,
        uid: icon.uid,
        callback_message: icon.callback_message,
        version,
        reacted,
        events,
    })
}

/// The stored registration for `key`, re-enumerating Explorer's toolbars if
/// the stored copy is stale (first use, or the application re-registered
/// after a restart).
fn resolve_icon(key: &str) -> Result<TrayIconInfo, String> {
    if let Some(icon) = stored_icon(key) {
        if unsafe { IsWindow(HWND(icon.owner_hwnd as *mut _)) }.as_bool() {
            return Ok(icon);
        }
    }
    enumerate_tray_icons();
    stored_icon(key)
        .ok_or_else(|| "This tray icon's registration is no longer in Explorer's tray".to_owned())
}

/// Posts the version-4 event for `kind`: one message with the anchor
/// coordinates in `wParam`, the event in `LOWORD(lParam)` and the icon id in
/// `HIWORD(lParam)`.
fn post_version_4(icon: &TrayIconInfo, kind: ClickKind, anchor: POINT) -> &'static str {
    let event = match kind {
        ClickKind::Left => NIN_SELECT,
        ClickKind::Right => WM_CONTEXTMENU,
    };
    let owner = HWND(icon.owner_hwnd as *mut _);
    unsafe {
        let _ = PostMessageW(
            owner,
            icon.callback_message,
            WPARAM(pack_coords(anchor) as usize),
            LPARAM((event as u32 as isize) | ((icon.uid as isize) << 16)),
        );
    }
    match kind {
        ClickKind::Left => "NIN_SELECT",
        ClickKind::Right => "WM_CONTEXTMENU",
    }
}

/// Posts the legacy (versions 0/3) mouse-message pair for `kind`: icon id in
/// `wParam`, mouse message in `lParam`.
fn post_legacy(icon: &TrayIconInfo, kind: ClickKind) -> &'static str {
    let (down, up) = match kind {
        ClickKind::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP),
        ClickKind::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP),
    };
    let owner = HWND(icon.owner_hwnd as *mut _);
    unsafe {
        let _ = PostMessageW(
            owner,
            icon.callback_message,
            WPARAM(icon.uid as usize),
            LPARAM(down as isize),
        );
    }
    std::thread::sleep(LEGACY_PAIR_GAP);
    unsafe {
        let _ = PostMessageW(
            owner,
            icon.callback_message,
            WPARAM(icon.uid as usize),
            LPARAM(up as isize),
        );
    }
    match kind {
        ClickKind::Left => "WM_LBUTTONDOWN+WM_LBUTTONUP",
        ClickKind::Right => "WM_RBUTTONDOWN+WM_RBUTTONUP",
    }
}

/// The anchor for version-4 messages: the user's real cursor position —
/// where the click physically happened in the custom taskbar, exactly where
/// Explorer would have placed the menu. Read-only; never moves the cursor.
fn cursor_anchor() -> POINT {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        return point;
    }
    // Fall back to the bottom-right of the primary screen if the cursor
    // cannot be read.
    let (x, y) = unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) - 64,
            GetSystemMetrics(SM_CYSCREEN) - 64,
        )
    };
    POINT { x, y }
}

/// Packs screen coordinates like a mouse message's `wParam`/anchor:
/// X in the low word, Y in the high word.
fn pack_coords(point: POINT) -> i32 {
    (point.y as i16 as i32) << 16 | (point.x as i16 as i32)
}

fn cached_version(key: &str) -> Option<u32> {
    ICON_VERSIONS
        .lock()
        .expect("tray version cache lock poisoned")
        .get(key)
        .copied()
}

fn cache_version(key: &str, version: u32) {
    ICON_VERSIONS
        .lock()
        .expect("tray version cache lock poisoned")
        .insert(key.to_owned(), version);
}

/// Whether the application reacted: its thread entered menu mode (its own
/// native context menu is up) or a new visible window appeared.
fn wait_for_reaction(thread_id: u32, pid: Option<u32>, before: &HashSet<isize>) -> bool {
    let started = Instant::now();
    while started.elapsed() < REACTION_TIMEOUT {
        std::thread::sleep(Duration::from_millis(60));
        if menu_mode_active(thread_id)
            || pid
                .map(|pid| !visible_windows_of(Some(pid)).is_subset(before))
                .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

fn menu_mode_active(thread_id: u32) -> bool {
    if thread_id == 0 {
        return false;
    }
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread_id, &mut info) }.is_err() {
        return false;
    }
    let flags = info.flags.0;
    flags & (GUI_INMENUMODE | GUI_SYSTEMMENUMODE | GUI_POPUPMENUMODE) != 0
        || !info.hwndMenuOwner.0.is_null()
}

fn visible_windows_of(pid: Option<u32>) -> HashSet<isize> {
    let Some(pid) = pid else {
        return HashSet::new();
    };
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_windows),
            LPARAM(&mut list as *mut Vec<HWND> as isize),
        );
    }
    list.into_iter()
        .filter(|hwnd| {
            window_pid(*hwnd) == Some(pid) && unsafe { IsWindowVisible(*hwnd) }.as_bool()
        })
        .map(|hwnd| hwnd.0 as isize)
        .collect()
}

unsafe extern "system" fn collect_windows(
    hwnd: HWND,
    lparam: LPARAM,
) -> windows::Win32::Foundation::BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    true.into()
}

fn concat_events(first: &'static str, second: &'static str) -> &'static str {
    match (first, second) {
        ("NIN_SELECT", "WM_LBUTTONDOWN+WM_LBUTTONUP") => "NIN_SELECT,WM_LBUTTONDOWN+WM_LBUTTONUP",
        ("WM_CONTEXTMENU", "WM_RBUTTONDOWN+WM_RBUTTONUP") => {
            "WM_CONTEXTMENU,WM_RBUTTONDOWN+WM_RBUTTONUP"
        }
        _ => "multiple",
    }
}
