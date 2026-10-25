use super::{icons, TaskbarApp};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, RECT, WPARAM},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    System::Threading::{
        CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetCursorInfo, GetForegroundWindow, GetWindowLongW,
        GetWindowPlacement, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, DispatchMessageW, GetMessageW, IsWindowVisible,
        PostMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, GWL_EXSTYLE, SW_MINIMIZE,
        SW_RESTORE, SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WM_CLOSE, WINEVENT_OUTOFCONTEXT,
        WS_EX_TOOLWINDOW, EVENT_SYSTEM_FOREGROUND, CURSORINFO, CURSOR_SHOWING,
    },
    UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
};

/// The last non-Glassbar window that held foreground, recorded the moment it
/// changed (EVENT_SYSTEM_FOREGROUND hook). Clicking the taskbar overlay makes
/// Glassbar itself the foreground window BEFORE the click's activate command
/// runs, so the minimize-toggle needs this record to know whether the clicked
/// app was the one in focus just before the click.
static LAST_FOREGROUND_APP: AtomicIsize = AtomicIsize::new(0);
/// The overlay window's hwnd, set once at startup: its own foreground
/// activations (every taskbar click) must not disturb the tracking.
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
/// Whether a Glassbar window OTHER than the overlay (the settings window)
/// was the last thing to take foreground — while it did, no app was in
/// focus, so a taskbar click must never minimize.
static FOCUS_ON_GLASSBAR: AtomicBool = AtomicBool::new(false);

/// Whether the given window belongs to Glassbar itself (its overlay/settings
/// windows) — those never count as the "app in focus" for the toggle.
fn is_glassbar_window(hwnd: HWND) -> bool {
    window_process_image(hwnd.0 as isize)
        .and_then(|(_, image)| {
            Path::new(&image)
                .file_stem()
                .and_then(|name| name.to_str())
                .map(|name| name.to_owned())
        })
        .is_some_and(|name| name.eq_ignore_ascii_case("glassbar"))
}

/// Whether an app window was in focus just before the taskbar click: the
/// clicked window itself, with no Glassbar window (settings) having taken
/// focus since.
pub fn was_last_foreground_app(window_id: isize) -> bool {
    !FOCUS_ON_GLASSBAR.load(Ordering::Relaxed)
        && LAST_FOREGROUND_APP.load(Ordering::Relaxed) == window_id
}

/// Installs the EVENT_SYSTEM_FOREGROUND hook on a dedicated thread with a
/// message pump (out-of-context hooks need one). Called once at startup with
/// the overlay's hwnd.
pub fn start_foreground_tracker(overlay_hwnd: isize) {
    OVERLAY_HWND.store(overlay_hwnd, Ordering::Relaxed);
    std::thread::spawn(|| {
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            eprintln!("[foreground] could not install the foreground hook");
            return;
        }
        let mut message = Default::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        unsafe {
            let _ = UnhookWinEvent(hook);
        };
    });
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.0.is_null() || hwnd.0 == OVERLAY_HWND.load(Ordering::Relaxed) as *mut _ {
        // The overlay taking foreground is just the taskbar click itself.
        return;
    }
    if is_glassbar_window(hwnd) {
        // The settings window (or another Glassbar surface) took focus:
        // no app is in focus while it does.
        FOCUS_ON_GLASSBAR.store(true, Ordering::Relaxed);
        return;
    }
    if is_shell_surface(hwnd) {
        // The desktop (Progman/WorkerW) or a shell taskbar took focus:
        // no app is in focus then either.
        LAST_FOREGROUND_APP.store(0, Ordering::Relaxed);
        FOCUS_ON_GLASSBAR.store(false, Ordering::Relaxed);
        return;
    }
    LAST_FOREGROUND_APP.store(hwnd.0 as isize, Ordering::Relaxed);
    FOCUS_ON_GLASSBAR.store(false, Ordering::Relaxed);
}

/// Processes whose windows never get taskbar buttons: Glassbar itself (its
/// overlay/settings windows must not appear on its own taskbar) and system
/// surfaces that leak through the enumeration.
const PROCESS_BLACKLIST: [&str; 2] = ["glassbar", "textinputhost"];

/// Window titles that never get taskbar buttons regardless of process (the
/// Windows Input Experience keyboard host can appear under other processes).
const TITLE_BLACKLIST: [&str; 1] = ["Windows Input Experience"];

fn is_blacklisted(process_name: &str, title: &str) -> bool {
    PROCESS_BLACKLIST
        .iter()
        .any(|entry| process_name.eq_ignore_ascii_case(entry))
        || TITLE_BLACKLIST
            .iter()
            .any(|entry| title.eq_ignore_ascii_case(entry))
}

pub fn enumerate() -> Vec<TaskbarApp> {
    let mut results = Vec::new();
    let mut context = CollectContext {
        apps: &mut results,
        parents: icons::parent_map(),
    };
    unsafe {
        let context = LPARAM((&mut context as *mut CollectContext) as isize);
        let _ = EnumWindows(Some(collect_window), context);
    }
    results
}

/// Enumeration context shared by every `EnumWindows` callback: the collected
/// apps plus a single process-tree snapshot, so each window is resolved
/// against one parent map instead of taking a fresh one per window.
struct CollectContext<'a> {
    apps: &'a mut Vec<TaskbarApp>,
    parents: HashMap<u32, u32>,
}

/// Whether the window is a shell surface (wallpaper host or the real
/// taskbar) rather than an app window. These can hold foreground when
/// nothing else is focused, and their rects cover the whole screen — the
/// taskbar must never read them as fullscreen apps.
fn is_shell_surface(hwnd: HWND) -> bool {
    let mut class = [0u16; 24];
    let len = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
    let len = len.min(class.len());
    let class = String::from_utf16_lossy(&class[..len]);
    matches!(
        class.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

/// How much of the screen the foreground window covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ForegroundCover {
    /// The foreground window is maximized: it covers the work area above
    /// the taskbar strip, which the AppBar reservation protects.
    pub maximized: bool,
    /// The foreground window is true fullscreen: its rect covers the whole
    /// monitor edge to edge (games, video players, slideshows — windows
    /// that are not reported as maximized). A fullscreen surface ignores
    /// AppBar reservations entirely, so no reservation mode keeps the
    /// taskbar visible under it.
    pub fullscreen: bool,
}

/// Measures how much of the screen the foreground window covers — the
/// states in which an auto-hiding taskbar stays off-screen. Normal windows
/// leave the taskbar visible behind them instead.
pub fn foreground_cover() -> ForegroundCover {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || is_shell_surface(hwnd) {
            return ForegroundCover::default();
        }
        let mut cover = ForegroundCover::default();
        let mut placement = WINDOWPLACEMENT::default();
        if GetWindowPlacement(hwnd, &mut placement).is_ok()
            && placement.showCmd == SW_SHOWMAXIMIZED.0 as u32
        {
            cover.maximized = true;
        }
        // True fullscreen: the window rect covers its monitor edge to edge.
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return cover;
        }
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        )
        .as_bool()
        {
            return cover;
        }
        cover.fullscreen = rect.left <= monitor.rcMonitor.left
            && rect.top <= monitor.rcMonitor.top
            && rect.right >= monitor.rcMonitor.right
            && rect.bottom >= monitor.rcMonitor.bottom;
        cover
    }
}

/// Whether the mouse cursor is currently visible. Games hide the cursor
/// during gameplay, so a hidden cursor while a fullscreen app holds the
/// foreground means cursor motion is camera input, not a deliberate
/// taskbar reveal gesture.
pub fn cursor_visible() -> bool {
    let mut info = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetCursorInfo(&mut info) }
        .is_ok()
        && (info.flags.0 & CURSOR_SHOWING.0) != 0
}

/// Brings a window to the foreground, or — with `minimize_toggle` — plays
/// the taskbar's click toggle: the app that WAS in focus minimizes, anything
/// else (minimized, or visible but behind other windows) is raised and
/// focused. Focus is judged by the last recorded app foreground, not by
/// GetForegroundWindow at click time — by then the clicked overlay itself
/// holds foreground — nor by the (2s-stale) snapshot's active flag.
pub fn activate(window_id: isize, minimize_toggle: bool) -> Result<(), String> {
    let hwnd = HWND(window_id as *mut _);
    unsafe {
        if minimize_toggle {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                if !SetForegroundWindow(hwnd).as_bool() {
                    return Err("Windows rejected the foreground activation request".to_string());
                }
                return Ok(());
            }
            if was_last_foreground_app(window_id) {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                return Ok(());
            }
            if !SetForegroundWindow(hwnd).as_bool() {
                return Err("Windows rejected the foreground activation request".to_string());
            }
            return Ok(());
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if SetForegroundWindow(hwnd).as_bool() {
            Ok(())
        } else {
            Err("Windows rejected the foreground activation request".to_string())
        }
    }
}

/// Politely asks the window to close (same as the title bar's X button).
pub fn close(window_id: isize) -> Result<(), String> {
    let hwnd = HWND(window_id as *mut _);
    unsafe {
        PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0))
            .map_err(|error| format!("Could not close the window: {error}"))
    }
}

/// The executable path of the process that owns the given window, resolved
/// through the process tree (see `main_process_image`) so helper-owned
/// windows pin and launch the app that can actually be started.
pub fn executable_path_for_window(window_id: isize) -> Option<String> {
    let (process_id, image) = window_process_image(window_id)?;
    let parents = icons::parent_map();
    Some(main_process_image(process_id, &image, &parents))
}

/// The process id owning the given window, plus its raw executable image
/// path (no process-tree resolution).
fn window_process_image(window_id: isize) -> Option<(u32, String)> {
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(HWND(window_id as *mut _), Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }
    let wide = icons::executable_path(process_id)?;
    Some((process_id, String::from_utf16_lossy(&wide[..wide.len() - 1])))
}

/// Parts of an image filename that mark a helper process: a piece of an app
/// (its web UI, an embedded WebView2 host, a crash reporter) that owns app
/// windows but cannot launch the app itself when run.
const HELPER_NAME_PARTS: [&str; 4] = ["helper", "webview", "crashreporter", "crashpad"];

fn is_helper_image(image: &str) -> bool {
    let name = Path::new(image)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['_', '-'], "");
    HELPER_NAME_PARTS
        .iter()
        .any(|part| name.contains(&part.to_ascii_lowercase().replace(['_', '-'], "")))
}

/// Windows system directories: a process running from there is a shell host
/// (Explorer, svchost...), never the app a user would launch, so walking up
/// the process tree must stop before it.
fn is_system_image(image: &str) -> bool {
    let lower = image.to_ascii_lowercase();
    lower.starts_with("c:\\windows\\") || lower.starts_with("\\??\\c:\\windows\\")
}

/// The launchable app behind a window's owning process: helper-window
/// processes (Steam's steamwebhelper, the WebView2 host embedded in other
/// apps) are children of the exe a user would actually run, so walk up the
/// parent chain to the oldest ancestor that is neither a Windows system
/// process nor Glassbar itself (which launches pins and must not be adopted
/// as their parent). Non-helper images are returned unchanged.
fn main_process_image(process_id: u32, image: &str, parents: &HashMap<u32, u32>) -> String {
    let mut resolved = image.to_string();
    if !is_helper_image(image) {
        return resolved;
    }
    let self_stem = std::env::current_exe().ok().and_then(|path| {
        Path::new(&path)
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    });
    let mut current = process_id;
    for _ in 0..16 {
        let Some(&parent) = parents.get(&current) else {
            break;
        };
        if parent == 0 || parent == current {
            break;
        }
        let Some(wide) = icons::executable_path(parent) else {
            break;
        };
        let parent_image = String::from_utf16_lossy(&wide[..wide.len() - 1]);
        if is_system_image(&parent_image) {
            break;
        }
        let Some(parent_stem) = Path::new(&parent_image)
            .file_stem()
            .and_then(|name| name.to_str())
        else {
            break;
        };
        if self_stem
            .as_deref()
            .is_some_and(|own| parent_stem.eq_ignore_ascii_case(own))
        {
            break;
        }
        current = parent;
        resolved = parent_image;
    }
    resolved
}

/// Name and icon for a window's owning process, resolved through the process
/// tree so helper-owned windows (Steam's web UI, embedded webviews) are named
/// and iconized after the app that would be launched for them.
fn app_visual(process_id: u32, parents: &HashMap<u32, u32>) -> icons::ApplicationVisual {
    let Some(wide) = icons::executable_path(process_id) else {
        return icons::ApplicationVisual {
            process_name: "Application".to_string(),
            icon: None,
        };
    };
    let image = String::from_utf16_lossy(&wide[..wide.len() - 1]);
    let resolved = main_process_image(process_id, &image, parents);
    icons::ApplicationVisual {
        process_name: Path::new(&resolved)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Application")
            .to_string(),
        icon: icons::icon_for_path(&resolved),
    }
}

/// Launches an executable path (used by pinned apps that aren't running).
pub fn launch_executable(path: &str) -> Result<(), String> {
    // The command line must be quoted: paths with spaces would otherwise be
    // split by CreateProcessW's argument parsing.
    let mut command: Vec<u16> = format!("\"{path}\"").encode_utf16().collect();
    command.push(0);
    let mut application: Vec<u16> = path.encode_utf16().collect();
    application.push(0);
    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            windows::core::PCWSTR(application.as_ptr()),
            windows::core::PWSTR(command.as_mut_ptr()),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            None,
            &startup,
            &mut process,
        )
        .map_err(|error| format!("Could not launch {path}: {error}"))?;
    }
    Ok(())
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
        return BOOL(1);
    }

    if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0) != 0 {
        return BOOL(1);
    }

    let title_length = GetWindowTextLengthW(hwnd) as usize;
    let mut title_buffer = vec![0_u16; title_length + 1];
    let copied = GetWindowTextW(hwnd, &mut title_buffer);
    if copied == 0 {
        return BOOL(1);
    }
    let title = String::from_utf16_lossy(&title_buffer[..copied as usize]);

    if is_blacklisted("", &title) {
        return BOOL(1);
    }

    let mut process_id = 0;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    let context = &mut *(lparam.0 as *mut CollectContext);
    let visual = app_visual(process_id, &context.parents);

    if is_blacklisted(&visual.process_name, &title) {
        return BOOL(1);
    }

    context.apps.push(TaskbarApp {
        id: hwnd.0 as isize,
        title,
        process_name: visual.process_name,
        icon: visual.icon,
        is_active: hwnd == GetForegroundWindow(),
        is_minimized: IsIconic(hwnd).as_bool(),
    });
    BOOL(1)
}
