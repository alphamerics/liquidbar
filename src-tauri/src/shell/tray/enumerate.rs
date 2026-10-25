//! Tray icon enumeration.
//!
//! Two layers, both reading the SAME source of truth — the shell's tray
//! service inside the (untouched, running) Explorer process:
//!
//! * **Registration layer (every shell)**: each notification-area icon's
//!   `Shell_NotifyIcon` registration is held by Explorer as a record whose
//!   first fields are exactly the classic registration triple — owner
//!   window, icon id, callback message. The records are located by scanning
//!   Explorer's private memory for the `{HWND, uID}` pair of every live
//!   window, and each record's callback message is its first
//!   16-bit-plausible message id. This is read-only memory access, works
//!   with the tray UI hidden or closed, and needs no input, accessibility,
//!   screen coordinates or screen scraping.
//! * **Toolbar layer (classic shells)**: Explorer hosts the icons as buttons
//!   in its tray toolbars — the live tray's (`Shell_TrayWnd` /
//!   `TrayNotifyWnd`) and the classic overflow flyout's
//!   (`NotifyIconOverflowWindow`). The buttons expose the registration
//!   triple through `TBBUTTON.dwData`, the registered tooltip through
//!   `TB_GETBUTTONTEXTW` and the icon image through the toolbar's image
//!   list. Where a modern (Windows 11) shell keeps these toolbars empty,
//!   this layer simply contributes nothing.
//!
//! Every icon's callback metadata is stored once at enumeration time; the
//! forwarding module replays messages from this stored registry.
//!
//! Enumeration results are cached: while the live-window environment (the
//! `(handle, pid)` set of every window) is unchanged and the enumeration is
//! recent, callers get the cached list and no memory walk, shell probe or
//! icon extraction runs at all. Icon images are cached separately with their
//! own lifetime, so re-scans do not re-encode PNGs for icons that have not
//! changed.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT,
    MEM_PRIVATE, MEM_RELEASE, MEM_RESERVE, PAGE_PROTECTION_FLAGS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE,
};
use windows::Win32::UI::Controls::{
    ImageList_GetIcon, ILD_NORMAL, TBBUTTON, TB_BUTTONCOUNT, TB_GETBUTTON, TB_GETBUTTONTEXTW,
    TB_GETIMAGELIST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumChildWindows, EnumWindows, FindWindowExW, GetWindowTextLengthW,
    GetWindowTextW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_NORMAL,
};

/// One enumerated notification-area icon, ready for the custom tray UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayIconInfo {
    /// Stable interaction key: `"<owner-hwnd-hex>:<uid>"`.
    pub key: String,
    /// The window that owns the `Shell_NotifyIcon` registration and receives
    /// the callback messages.
    pub owner_hwnd: isize,
    /// The registration's icon id.
    pub uid: u32,
    /// The callback message id the shell posts to the owner window.
    pub callback_message: u32,
    /// The owning application's display name (prettified executable stem).
    pub process_name: String,
    /// The registered tooltip/title: the toolbar's button text, else the
    /// owner window's title.
    pub tooltip: Option<String>,
    /// The icon image as a PNG data URL: the toolbar's image-list bitmap,
    /// else the owning executable's icon.
    pub icon: Option<String>,
}

impl TrayIconInfo {
    fn key(owner_hwnd: isize, uid: u32) -> String {
        format!("{owner_hwnd:x}:{uid:x}")
    }
}

/// The cached enumeration: the full icon list, the live-window signature it
/// was built against, and when it was built. The full scan (Explorer memory
/// walk + per-record shell probe + icon extraction) is expensive, so it only
/// re-runs when the window environment changed or the cache aged out.
struct TrayCache {
    at: Instant,
    signature: u64,
    icons: Vec<TrayIconInfo>,
}

/// How long a full enumeration stays authoritative even with an unchanged
/// window environment. An application can add or remove a tray icon without
/// creating or destroying a window, so the signature alone cannot detect
/// that; the TTL bounds how stale such a change can render.
const CACHE_TTL: Duration = Duration::from_secs(10);

/// The stored registry: callback metadata for every icon the shell currently
/// holds. Filled by [`enumerate_tray_icons`], reused by the forwarding
/// module for all future interactions.
static TRAY_CACHE: Mutex<Option<TrayCache>> = Mutex::new(None);

/// Enumerates every notification-area icon the shell holds and replaces the
/// stored registry. Returns the fresh list — or the cached one, while the
/// live-window environment is unchanged and the enumeration is recent.
pub fn enumerate_tray_icons() -> Vec<TrayIconInfo> {
    let signature = environment_signature(&live_window_handles());
    {
        let cache = TRAY_CACHE.lock().expect("tray registry lock poisoned");
        if let Some(cached) = cache.as_ref() {
            if cached.signature == signature && cached.at.elapsed() < CACHE_TTL {
                return cached.icons.clone();
            }
        }
    }
    let mut icons = scan_explorer_registrations();
    merge_toolbar_metadata(&mut icons);
    prune_icon_cache();
    let mut cache = TRAY_CACHE.lock().expect("tray registry lock poisoned");
    *cache = Some(TrayCache {
        at: Instant::now(),
        signature,
        icons: icons.clone(),
    });
    icons
}

/// The stored metadata for one icon, by interaction key.
pub fn stored_icon(key: &str) -> Option<TrayIconInfo> {
    TRAY_CACHE
        .lock()
        .expect("tray registry lock poisoned")
        .as_ref()?
        .icons
        .iter()
        .find(|icon| icon.key == key)
        .cloned()
}

// ---------------------------------------------------------------------------
// Icon image cache
// ---------------------------------------------------------------------------

/// Why an icon was extracted: from a tray toolbar's image list (the icon
/// the shell itself displays) or from the owning executable. Both paths may
/// run for the same icon, so the cache keys keep them apart.
const ICON_SOURCE_TOOLBAR: u8 = 0;
const ICON_SOURCE_EXECUTABLE: u8 = 1;

/// A cached icon image, with the time it was extracted. Icons are PNG data
/// URLs (GDI reads + PNG encode + base64 per extraction — hundreds of
/// microseconds to milliseconds each), so repeated scans reuse them.
struct CachedIcon {
    at: Instant,
    icon: Option<String>,
}

/// Keyed by `(owner hwnd, owner pid, uid, source)`: the pid keeps entries
/// from leaking across window-handle reuse (a dead window's handle value can
/// be handed to a different process later).
type IconKey = (isize, u32, u32, u8);

static ICON_CACHE: LazyLock<Mutex<HashMap<IconKey, CachedIcon>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How long a cached icon image stays authoritative (applications can change
/// their tray icon at runtime).
const ICON_TTL: Duration = Duration::from_secs(30);

/// Entries older than this are dropped outright (their windows are almost
/// certainly gone; handle reuse makes unbounded growth unwise).
const ICON_CACHE_PRUNE: Duration = Duration::from_secs(300);

/// The icon image for one registration, from the cache when fresh, extracted
/// (and stored) otherwise. Extraction runs OUTSIDE the lock: it is slow GDI
/// and shell work, and concurrent scans just duplicate it harmlessly.
fn cached_icon(key: IconKey, extract: impl FnOnce() -> Option<String>) -> Option<String> {
    {
        let cache = ICON_CACHE.lock().expect("icon cache lock poisoned");
        if let Some(entry) = cache.get(&key) {
            if entry.at.elapsed() < ICON_TTL {
                return entry.icon.clone();
            }
        }
    }
    let icon = extract();
    ICON_CACHE
        .lock()
        .expect("icon cache lock poisoned")
        .insert(key, CachedIcon {
            at: Instant::now(),
            icon: icon.clone(),
        });
    icon
}

/// Drops icon-cache entries nothing has refreshed for a while.
fn prune_icon_cache() {
    ICON_CACHE
        .lock()
        .expect("icon cache lock poisoned")
        .retain(|_, entry| entry.at.elapsed() < ICON_CACHE_PRUNE);
}

// ---------------------------------------------------------------------------
// Registration layer: Explorer's tray service records
// ---------------------------------------------------------------------------

/// Window-message-id space: tray callbacks are registered by applications in
/// the `WM_USER`..`WM_APP`..registered-message ranges (0x0400..0xFFFF);
/// anything smaller would collide with system messages.
const CALLBACK_MIN: u32 = 0x0400;
const CALLBACK_MAX: u32 = 0xFFFF;

/// How far past the `{HWND, uID}` pair the record is searched for its
/// callback message.
const CALLBACK_SEARCH_BYTES: usize = 256;

/// How much memory is scanned at most per enumeration (a safety cap well
/// above Explorer's total private commit).
const SCAN_MEMORY_BUDGET: usize = 8 * 1024 * 1024 * 1024;

/// Overlap between consecutive region chunks so records straddling a chunk
/// boundary are still found.
const CHUNK_OVERLAP: usize = 256;

/// Enumerates every icon registration from Explorer's tray service records,
/// confirmed against the shell, plus framework-signature registrations the
/// shell's records cannot represent (GUID-less Electron hosts).
fn scan_explorer_registrations() -> Vec<TrayIconInfo> {
    let owners = live_window_owners();
    let mut icons: Vec<TrayIconInfo> = Vec::new();
    let mut seen: HashSet<(usize, u32)> = HashSet::new();

    if let Some(explorer_pid) = explorer_pid() {
        if let Ok(explorer) = unsafe {
            OpenProcess(
                PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                explorer_pid,
            )
        } {
            let mut raw: Vec<FoundRecord> = Vec::new();
            for record in scan_records(explorer, &owners) {
                // Glassbar's own icons must not appear in its own tray UI.
                if owners
                    .get(&(record.owner_hwnd as u64))
                    .map(|pid| *pid == std::process::id())
                    .unwrap_or(true)
                {
                    continue;
                }
                raw.push(record);
            }
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(explorer) };
            raw.sort_by_key(|record| (record.owner_hwnd, record.uid));

            // The same registration may exist in more than one copy
            // (service + view model). Keep one per (owner, uID): the
            // compact `{HWND, uID, uCallbackMessage}` layout is the
            // definitive registration signature and wins over extended
            // copies.
            let compact: HashSet<(usize, u32)> = raw
                .iter()
                .filter(|record| record.callback_offset == 12)
                .map(|record| (record.owner_hwnd, record.uid))
                .collect();

            for &record in &raw {
                if !seen.insert((record.owner_hwnd, record.uid)) {
                    continue;
                }
                if record.callback_offset != 12
                    && compact.contains(&(record.owner_hwnd, record.uid))
                {
                    continue;
                }
                // Confirm the (HWND, uID) pair against the shell: a live
                // registration returns Ok (even while the icon is parked in
                // the closed hidden-icon flyout). False record-shaped
                // patterns (IME/IPC internals) fail and are dropped.
                if !shell_knows(record.owner_hwnd as isize, record.uid) {
                    continue;
                }
                let owner_hwnd = record.owner_hwnd as isize;
                icons.push(TrayIconInfo {
                    key: TrayIconInfo::key(owner_hwnd, record.uid),
                    owner_hwnd,
                    uid: record.uid,
                    callback_message: record.callback,
                    process_name: process_display_name(HWND(owner_hwnd as *mut _)),
                    tooltip: owner_window_title(HWND(owner_hwnd as *mut _)),
                    icon: None,
                });
            }
        }
    }

    electron_registrations(&mut icons, &mut seen);
    icons
}

/// Whether the shell currently holds a registration for `(hwnd, uid)` —
/// documented and read-only; works while the icon's tray area is hidden or
/// the icon is parked in the closed flyout.
fn shell_knows(hwnd: isize, uid: u32) -> bool {
    let identifier = windows::Win32::UI::Shell::NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<windows::Win32::UI::Shell::NOTIFYICONIDENTIFIER>() as u32,
        hWnd: HWND(hwnd as *mut _),
        uID: uid,
        guidItem: windows::core::GUID::zeroed(),
    };
    unsafe { windows::Win32::UI::Shell::Shell_NotifyIconGetRect(&identifier) }.is_ok()
}

// ---------------------------------------------------------------------------
// Framework signatures: Electron's tray host
// ---------------------------------------------------------------------------

/// Electron hosts every tray icon on one hidden window of class
/// `Electron_NotifyIconHostWindow`, with a fixed callback message
/// (`WM_APP + 1`) and sequential icon ids starting at 2 (see Electron's
/// `notify_icon_host.cc` — its WndProc answers unknown icon ids with `TRUE`
/// too, so the number of live icons is not observable from outside). The
/// first icon (uid 2) is emitted; the event format is the legacy one — the
/// mouse message arrives directly in `lParam` — which the forwarding
/// module's legacy path delivers.
fn electron_registrations(icons: &mut Vec<TrayIconInfo>, seen: &mut HashSet<(usize, u32)>) {
    const ELECTRON_HOST_CLASS: &str = "Electron_NotifyIconHostWindow";
    const ELECTRON_CALLBACK: u32 = 0x8001; // WM_APP + 1
    const ELECTRON_BASE_UID: u32 = 2;

    let mut hosts: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut hosts as *mut Vec<HWND> as isize),
        );
    }
    for host in hosts {
        if super::window_pid(host) == Some(std::process::id()) {
            continue;
        }
        if !super::window_class(host)
            .map(|class| class.eq_ignore_ascii_case(ELECTRON_HOST_CLASS))
            .unwrap_or(false)
        {
            continue;
        }
        let uid = ELECTRON_BASE_UID;
        let owner_hwnd = host.0 as isize;
        if seen.insert((owner_hwnd as usize, uid)) {
            icons.push(TrayIconInfo {
                key: TrayIconInfo::key(owner_hwnd, uid),
                owner_hwnd,
                uid,
                callback_message: ELECTRON_CALLBACK,
                process_name: process_display_name(host),
                tooltip: owner_window_title(host),
                icon: None,
            });
        }
    }
}

/// A found record's callback offset (for cross-copy consistency).
#[derive(Clone, Copy, Debug)]
struct FoundRecord {
    owner_hwnd: usize,
    uid: u32,
    callback: u32,
    callback_offset: usize,
}

/// Walks Explorer's committed private memory and yields every `{HWND, uID,
/// ...callback}` record whose HWND is a live window.
fn scan_records(
    explorer: windows::Win32::Foundation::HANDLE,
    owners: &HashMap<u64, u32>,
) -> Vec<FoundRecord> {
    let low_dwords: HashSet<u32> = owners
        .keys()
        .filter_map(|hwnd| {
            let low = (*hwnd & 0xFFFF_FFFF) as u32;
            (low != 0).then_some(low)
        })
        .collect();

    let mut records = Vec::new();
    let mut scanned = 0usize;
    let mut address = 0usize;
    // One reusable read buffer: allocating (and zeroing) a fresh 16 MB
    // buffer per chunk made a multi-gigabyte scan spend most of its time in
    // the allocator.
    let mut buffer = vec![0u8; 16 * 1024 * 1024 + CHUNK_OVERLAP];
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        if unsafe {
            VirtualQueryEx(
                explorer,
                Some(address as *const core::ffi::c_void),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } == 0
        {
            break;
        }
        let base = info.BaseAddress as usize;
        let region = info.RegionSize as usize;
        if region == 0 || base + region <= address {
            break;
        }
        if scanned > SCAN_MEMORY_BUDGET {
            break;
        }
        if info.State == MEM_COMMIT && info.Type == MEM_PRIVATE && readable(info.Protect) {
                let mut chunk_base = base;
                let mut remaining = region;
                while remaining > 0 {
                    let take = remaining.min(16 * 1024 * 1024);
                    // Reads stay inside the region; the next chunk re-reads the
                    // overlap so records straddling a chunk boundary are found.
                    let read_size = if take < remaining {
                        take + CHUNK_OVERLAP
                    } else {
                        take
                    };
                    let mut read = 0usize;
                    if unsafe {
                        ReadProcessMemory(
                            explorer,
                            chunk_base as *const core::ffi::c_void,
                            buffer.as_mut_ptr().cast(),
                            read_size,
                            Some(&mut read),
                        )
                    }
                    .is_err()
                        || read <= 16
                    {
                        break;
                    }
                    scanned += take;
                    collect_records(&buffer[..read], owners, &low_dwords, &mut records);
                if take >= remaining {
                    break;
                }
                chunk_base += take - CHUNK_OVERLAP;
                remaining -= take - CHUNK_OVERLAP;
            }
        }
        address = base + region;
    }
    records
}

/// Whether a memory region's protection allows ReadProcessMemory.
fn readable(protect: PAGE_PROTECTION_FLAGS) -> bool {
    // PAGE_NOACCESS (1) and the guard/reserved variants are unreadable;
    // every regular readable protection is allowed.
    const NOACCESS: u32 = 0x01;
    const GUARD: u32 = 0x100;
    let bits = protect.0;
    bits != NOACCESS && bits & GUARD == 0 && bits != 0
}

/// Finds every record pattern in one region chunk. Two record shapes carry
/// the registration triple `{HWND, uID, uCallbackMessage}`:
///
/// * **Compact** — the classic tray registration: the callback message sits
///   directly after the uID (offset +12), as the shell's own toolbars
///   expose it.
/// * **Extended** — the modern tray service's record: a zero block after the
///   uID, then the callback message as the first 16-bit-plausible message id
///   in the record, sitting in its own qword field (followed by a zero
///   dword — a pointer's high dword, the systematic false positive, is
///   always followed by the pointer's nonzero low dword instead).
fn collect_records(
    buffer: &[u8],
    owners: &HashMap<u64, u32>,
    low_dwords: &HashSet<u32>,
    records: &mut Vec<FoundRecord>,
) {
    if buffer.len() < CALLBACK_SEARCH_BYTES + 16 {
        return;
    }
    let limit = buffer.len() - CALLBACK_SEARCH_BYTES - 16;
    let mut offset = 0usize;
    while offset <= limit {
        // Prefilter: live HWNDs are small values (window handle table), so
        // the high dword is zero — this rejects pointer qwords instantly.
        if buffer[offset + 4] != 0
            || buffer[offset + 5] != 0
            || buffer[offset + 6] != 0
            || buffer[offset + 7] != 0
        {
            offset += 4;
            continue;
        }
        let low = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
        if !low_dwords.contains(&low) {
            offset += 4;
            continue;
        }
        let hwnd = u64::from_le_bytes(buffer[offset..offset + 8].try_into().unwrap());
        if !owners.contains_key(&hwnd) {
            offset += 4;
            continue;
        }
        let uid = u32::from_le_bytes(buffer[offset + 8..offset + 12].try_into().unwrap());
        if uid > CALLBACK_MAX {
            offset += 4;
            continue;
        }
        // Compact layout: callback directly after the uID.
        let compact = u32::from_le_bytes(buffer[offset + 12..offset + 16].try_into().unwrap());
        if plausible_callback(compact) {
            records.push(FoundRecord {
                owner_hwnd: hwnd as usize,
                uid,
                callback: compact,
                callback_offset: 12,
            });
            offset += 16;
            continue;
        }
        // Extended layout: a zero block after the uID, then the callback as
        // the record's first 16-bit-plausible message id.
        let zeros = (0..5).all(|index| {
            u32::from_le_bytes(
                buffer[offset + 12 + index * 4..offset + 16 + index * 4]
                    .try_into()
                    .unwrap(),
            ) == 0
        });
        if !zeros {
            offset += 4;
            continue;
        }
        let end = (offset + CALLBACK_SEARCH_BYTES).min(buffer.len() - 8);
        let mut search = offset + 32;
        let mut found_callback = None;
        while search <= end {
            let value = u32::from_le_bytes(buffer[search..search + 4].try_into().unwrap());
            if plausible_callback(value)
                && u32::from_le_bytes(buffer[search + 4..search + 8].try_into().unwrap()) == 0
            {
                found_callback = Some((value, search - offset));
                break;
            }
            search += 4;
        }
        if let Some((callback, callback_offset)) = found_callback {
            records.push(FoundRecord {
                owner_hwnd: hwnd as usize,
                uid,
                callback,
                callback_offset,
            });
            offset += callback_offset + 8;
            continue;
        }
        offset += 4;
    }
}

/// Whether a value is in the window-message-id space applications register
/// tray callbacks in (`WM_USER`..`WM_APP`..registered messages), excluding
/// the 0x7FFx block where Windows' 64-bit pointer high dwords live — the
/// systematic false positive.
fn plausible_callback(value: u32) -> bool {
    (CALLBACK_MIN..=CALLBACK_MAX).contains(&value) && !(0x7FF0..=0x7FFF).contains(&value)
}

/// Every live window's handle mapped to its process id: top-level windows
/// plus message-only windows (tray icon owners are one of the two).
fn live_window_owners() -> HashMap<u64, u32> {
    live_window_handles().into_iter().collect()
}

/// Every live window as `(handle, owner pid)`: top-level windows plus
/// message-only windows (tray icon owners are one of the two).
fn live_window_handles() -> Vec<(u64, u32)> {
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut list as *mut Vec<HWND> as isize),
        );
        let _ = EnumChildWindows(
            windows::Win32::UI::WindowsAndMessaging::HWND_MESSAGE,
            Some(collect_window),
            LPARAM(&mut list as *mut Vec<HWND> as isize),
        );
    }
    list.into_iter()
        .filter_map(|hwnd| super::window_pid(hwnd).map(|pid| (hwnd.0 as u64, pid)))
        .collect()
}

/// A cheap fingerprint of the window environment: the sorted `(handle, pid)`
/// pairs of every live window, hashed. Tray registrations live on windows,
/// so a changed signature means records may have appeared or vanished — the
/// signal that skips or triggers the expensive re-scan.
fn environment_signature(handles: &[(u64, u32)]) -> u64 {
    let mut sorted = handles.to_vec();
    sorted.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for pair in sorted {
        pair.hash(&mut hasher);
    }
    hasher.finish()
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    true.into()
}

fn explorer_pid() -> Option<u32> {
    unsafe {
        let tray = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::w!("Shell_TrayWnd"),
            windows::core::PCWSTR::null(),
        )
        .unwrap_or_default();
        if tray.0.is_null() {
            return None;
        }
        super::window_pid(tray)
    }
}

// ---------------------------------------------------------------------------
// Toolbar layer: classic tray toolbars (tooltip + icon image)
// ---------------------------------------------------------------------------

/// Top-level window classes that host a tray icon toolbar: the primary and
/// secondary taskbars' trays and the classic overflow flyout.
const TRAY_HOST_CLASSES: [&str; 3] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
];

/// The per-button registration block Explorer's tray toolbars expose through
/// `TBBUTTON.dwData`:
///
/// ```text
/// HWND hwnd; UINT uID; UINT uCallbackMessage; DWORD pad;
/// ```
const REGISTRATION_BYTES: usize = 16;

/// Tooltips are limited to 128 characters; one extra word of slack.
const TOOLTIP_WCHARS: usize = 260;

/// Enriches the registration layer's entries with the classic toolbars'
/// metadata: the registered tooltip (`TB_GETBUTTONTEXTW`) and the icon image
/// (the toolbar's image list). Also adds any registration the toolbars hold
/// that the memory scan missed (classic shells with the tray hidden).
fn merge_toolbar_metadata(icons: &mut Vec<TrayIconInfo>) {
    for toolbar in tray_toolbars() {
        for entry in read_toolbar_icons(toolbar) {
            match icons
                .iter_mut()
                .find(|icon| icon.owner_hwnd == entry.owner_hwnd && icon.uid == entry.uid)
            {
                Some(existing) => {
                    if existing.tooltip.is_none() {
                        existing.tooltip = entry.tooltip.clone();
                    }
                    if existing.icon.is_none() {
                        existing.icon = entry.icon.clone();
                    }
                }
                None => icons.push(entry),
            }
        }
    }
    // Fill in missing images with the owning executable's real icon.
    for entry in icons.iter_mut() {
        if entry.icon.is_none() {
            let owner_hwnd = entry.owner_hwnd;
            let uid = entry.uid;
            let pid = super::window_pid(HWND(owner_hwnd as *mut _)).unwrap_or(0);
            entry.icon = cached_icon(
                (owner_hwnd, pid, uid, ICON_SOURCE_EXECUTABLE),
                || executable_icon(owner_hwnd),
            );
        }
    }
}

/// Every existing tray toolbar: the live tray's one (where the toolbar sits
/// under `TrayNotifyWnd`) and the overflow flyout's one (direct child; the
/// flyout window exists and holds the hidden icons' buttons even while it is
/// not open on screen).
fn tray_toolbars() -> Vec<HWND> {
    let mut hosts: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_tray_hosts),
            LPARAM(&mut hosts as *mut Vec<HWND> as isize),
        );
    }
    let mut toolbars = Vec::new();
    for host in hosts {
        let direct = unsafe { child_by_class(host, "ToolbarWindow32") };
        if !direct.0.is_null() {
            toolbars.push(direct);
            continue;
        }
        let notify = unsafe { child_by_class(host, "TrayNotifyWnd") };
        if notify.0.is_null() {
            continue;
        }
        let toolbar = unsafe { child_by_class(notify, "ToolbarWindow32") };
        if !toolbar.0.is_null() {
            toolbars.push(toolbar);
        }
    }
    toolbars
}

unsafe extern "system" fn collect_tray_hosts(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    if let Some(class) = super::window_class(hwnd) {
        if TRAY_HOST_CLASSES
            .iter()
            .any(|host| class.eq_ignore_ascii_case(host))
        {
            list.push(hwnd);
        }
    }
    true.into()
}

unsafe fn child_by_class(parent: HWND, class: &str) -> HWND {
    let mut wide: Vec<u16> = class.encode_utf16().collect();
    wide.push(0);
    FindWindowExW(
        parent,
        HWND::default(),
        windows::core::PCWSTR(wide.as_ptr()),
        windows::core::PCWSTR::null(),
    )
    .unwrap_or(HWND::default())
}

/// Reads every icon registration out of one toolbar.
fn read_toolbar_icons(toolbar: HWND) -> Vec<TrayIconInfo> {
    let Some(explorer_pid) = super::window_pid(toolbar) else {
        return Vec::new();
    };
    // Glassbar's own icons must not appear in its own tray UI.
    if explorer_pid == std::process::id() {
        return Vec::new();
    }
    let Ok(process) = (unsafe {
        OpenProcess(
            PROCESS_VM_OPERATION
                | PROCESS_VM_READ
                | PROCESS_VM_WRITE
                | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            explorer_pid,
        )
    }) else {
        return Vec::new();
    };

    unsafe {
        let mut result = 0usize;
        let count = SendMessageTimeoutW(
            toolbar,
            TB_BUTTONCOUNT,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_NORMAL,
            1000,
            Some(&mut result),
        )
        .0 as usize;
        if count == 0 {
            return Vec::new();
        }

        // Remote memory in Explorer for the TBBUTTON and the tooltip text.
        let button_remote = VirtualAllocEx(
            process,
            None,
            std::mem::size_of::<TBBUTTON>(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        let text_remote = VirtualAllocEx(
            process,
            None,
            TOOLTIP_WCHARS * 2,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if button_remote.is_null() || text_remote.is_null() {
            if !button_remote.is_null() {
                let _ = VirtualFreeEx(process, button_remote, 0, MEM_RELEASE);
            }
            if !text_remote.is_null() {
                let _ = VirtualFreeEx(process, text_remote, 0, MEM_RELEASE);
            }
            return Vec::new();
        }

        // The toolbar's image list holds every icon's bitmap.
        let image_list = SendMessageTimeoutW(
            toolbar,
            TB_GETIMAGELIST,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_NORMAL,
            1000,
            Some(&mut result),
        )
        .0 as isize;

        let mut icons = Vec::new();
        for index in 0..count {
            if SendMessageTimeoutW(
                toolbar,
                TB_GETBUTTON,
                WPARAM(index),
                LPARAM(button_remote as isize),
                SMTO_ABORTIFHUNG | SMTO_NORMAL,
                1000,
                Some(&mut result),
            )
            .0 == 0
            {
                continue;
            }
            let mut button = TBBUTTON {
                iBitmap: 0,
                idCommand: 0,
                fsState: 0,
                fsStyle: 0,
                bReserved: [0; 6],
                dwData: 0,
                iString: 0,
            };
            let mut read = 0usize;
            if ReadProcessMemory(
                process,
                button_remote,
                (&mut button as *mut TBBUTTON).cast(),
                std::mem::size_of::<TBBUTTON>(),
                Some(&mut read),
            )
            .is_err()
                || button.dwData == 0
            {
                continue;
            }
            // The button data block starts with the registration:
            // HWND hwnd; UINT uID; UINT uCallbackMessage.
            let mut data = [0u8; REGISTRATION_BYTES];
            if ReadProcessMemory(
                process,
                button.dwData as *const core::ffi::c_void,
                data.as_mut_ptr().cast(),
                data.len(),
                Some(&mut read),
            )
            .is_err()
            {
                continue;
            }
            let owner_hwnd = isize::from_le_bytes(data[0..8].try_into().unwrap());
            let uid = u32::from_le_bytes(data[8..12].try_into().unwrap());
            let callback_message = u32::from_le_bytes(data[12..16].try_into().unwrap());
            if owner_hwnd == 0 || callback_message == 0 {
                continue;
            }
            // An icon owned by Glassbar itself is not part of its own tray.
            let owner_pid = super::window_pid(HWND(owner_hwnd as *mut _));
            if owner_pid == Some(std::process::id()) {
                continue;
            }

            let tooltip = read_tooltip(process, toolbar, button.idCommand, text_remote);
            let owner_pid = owner_pid.unwrap_or(0);
            let icon = cached_icon(
                (owner_hwnd, owner_pid, uid, ICON_SOURCE_TOOLBAR),
                || read_icon_image(image_list, button.iBitmap),
            )
            .or_else(|| {
                cached_icon(
                    (owner_hwnd, owner_pid, uid, ICON_SOURCE_EXECUTABLE),
                    || executable_icon(owner_hwnd),
                )
            });

            icons.push(TrayIconInfo {
                key: TrayIconInfo::key(owner_hwnd, uid),
                owner_hwnd,
                uid,
                callback_message,
                process_name: process_display_name(HWND(owner_hwnd as *mut _)),
                tooltip: tooltip.or_else(|| owner_window_title(HWND(owner_hwnd as *mut _))),
                icon,
            });
        }

        let _ = VirtualFreeEx(process, button_remote, 0, MEM_RELEASE);
        let _ = VirtualFreeEx(process, text_remote, 0, MEM_RELEASE);
        icons
    }
}

/// The button text is the icon's registered tooltip/title, read back from
/// Explorer's address space.
unsafe fn read_tooltip(
    process: windows::Win32::Foundation::HANDLE,
    toolbar: HWND,
    id_command: i32,
    text_remote: *mut core::ffi::c_void,
) -> Option<String> {
    let mut result = 0usize;
    let length = SendMessageTimeoutW(
        toolbar,
        TB_GETBUTTONTEXTW,
        WPARAM(id_command as usize),
        LPARAM(text_remote as isize),
        SMTO_ABORTIFHUNG | SMTO_NORMAL,
        1000,
        Some(&mut result),
    )
    .0 as usize;
    if length == 0 || length >= TOOLTIP_WCHARS {
        return None;
    }
    let mut wide = vec![0u16; length + 1];
    let mut read = 0usize;
    if ReadProcessMemory(
        process,
        text_remote,
        wide.as_mut_ptr().cast(),
        (length + 1) * 2,
        Some(&mut read),
    )
    .is_err()
    {
        return None;
    }
    let tooltip = String::from_utf16_lossy(&wide[..length]);
    let tooltip = tooltip.trim().to_string();
    if tooltip.is_empty() {
        None
    } else {
        Some(tooltip)
    }
}

/// The icon image: the button's bitmap from the toolbar's image list,
/// encoded as a PNG data URL for the custom tray UI.
unsafe fn read_icon_image(image_list: isize, bitmap_index: i32) -> Option<String> {
    if image_list == 0 || bitmap_index < 0 {
        return None;
    }
    let handle = windows::Win32::UI::Controls::HIMAGELIST(image_list);
    let icon = ImageList_GetIcon(handle, bitmap_index, ILD_NORMAL);
    if icon.0.is_null() {
        return None;
    }
    let encoded = crate::shell::icons::icon_as_png_data_url(icon);
    let _ = DestroyIcon(icon);
    encoded
}

// ---------------------------------------------------------------------------
// Owner metadata helpers
// ---------------------------------------------------------------------------

/// The owner window's title (many tray tooltips are built from it).
fn owner_window_title(hwnd: HWND) -> Option<String> {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if copied <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buffer[..copied as usize]);
    let title = title.trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// The owning application's display name for UI fallbacks.
fn process_display_name(owner: HWND) -> String {
    owner_executable(owner)
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .map(|stem| crate::shell::hidden::prettify_process(&stem))
        .unwrap_or_else(|| "Application".to_owned())
}

/// The owning executable's icon image, encoded as a PNG data URL.
fn executable_icon(owner: isize) -> Option<String> {
    let path = owner_executable(HWND(owner as *mut _))?;
    crate::shell::icons::icon_for_path(&path)
}

/// The owning window's executable path.
fn owner_executable(owner: HWND) -> Option<String> {
    let pid = super::window_pid(owner)?;
    let wide = crate::shell::icons::executable_path(pid)?;
    Some(String::from_utf16_lossy(
        &wide[..wide.len().saturating_sub(1)],
    ))
}
