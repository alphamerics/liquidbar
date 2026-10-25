//! Hidden background processes: top-level windows that exist but aren't on
//! screen — the tray-style background apps. This is the closest faithful
//! equivalent of Windows' hidden-icon area that the OS exposes on Windows 11
//! (the shell's own hidden-icon flyout is a XAML surface with no public read
//! API, and its legacy overflow toolbar is an empty stub).
//!
//! Only COMPLETELY hidden windows count: a minimized app has taskbar
//! presence (a real taskbar button), so it belongs there — not in this list.
//! Each entry carries the program's real icon (extracted from its
//! executable) and its real window title.

use serde::Serialize;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, SW_RESTORE, SW_SHOW,
};

/// Window classes that belong to the shell/desktop itself, never to a
/// background app.
const SHELL_CLASSES: [&str; 12] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "SystemTray_Main",
    "NotifyIconOverflowWindow",
    "XamlExplorerHostIslandWindow",
    "WindowsDashboard",
    "ForegroundStaging",
    "MSCTFIME UI",
    "IME",
    "tooltips_class32",
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenWindow {
    /// The window handle — the stable id for reveal.
    pub id: isize,
    /// The display name: the program itself (what the tray tooltip shows).
    pub name: String,
    /// The hidden window's internal title (context under the name).
    pub title: String,
    pub process_name: String,
    /// The program's real icon as a PNG data URL.
    pub icon: Option<String>,
}

/// A raw candidate collected during enumeration (icon extraction happens
/// once per surviving process afterwards).
struct Candidate {
    hwnd: isize,
    title: String,
    process_id: u32,
}

unsafe extern "system" fn collect_hidden(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<Candidate>);
    // Ourselves: the overlay, glass surfaces and settings window.
    if std::process::id() == unsafe { GetWindowThreadProcessId(hwnd, None) } {
        return true.into();
    }
    // A MINIMIZED window has taskbar presence — a real taskbar button — so
    // it belongs to the taskbar, not the hidden-icon area.
    if unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return true.into();
    }
    // Child windows and message-only stubs aren't reveal-able.
    let style =
        unsafe { GetWindowLongW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) } as u32;
    if style & windows::Win32::UI::WindowsAndMessaging::WS_CHILD.0 != 0 {
        return true.into();
    }
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = String::from_utf16_lossy(&class[..length.max(0) as usize]);
    if SHELL_CLASSES
        .iter()
        .any(|shell| class.eq_ignore_ascii_case(shell))
    {
        return true.into();
    }
    // Zero-size windows are notification stubs with nothing to reveal.
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return true.into();
    }
    let mut title_length = unsafe { GetWindowTextLengthW(hwnd) };
    if title_length <= 0 {
        return true.into();
    }
    title_length += 1;
    let mut title = vec![0u16; title_length as usize];
    unsafe { GetWindowTextW(hwnd, &mut title) };
    let title = String::from_utf16_lossy(&title[..(title_length - 1) as usize]);
    let title = title.trim().to_string();
    if title.is_empty() {
        return true.into();
    }
    let mut process_id = 0u32;
    let _ = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    list.push(Candidate {
        hwnd: hwnd.0 as isize,
        title,
        process_id,
    });
    true.into()
}

/// The background windows' list, one per process (with the program's real
/// icon), sorted by title.
pub fn hidden_windows() -> Result<Vec<HiddenWindow>, String> {
    let mut candidates: Vec<Candidate> = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect_hidden),
            LPARAM(&mut candidates as *mut _ as isize),
        )
        .map_err(|error| format!("Could not enumerate windows: {error}"))?;
    }
    let mut windows: Vec<HiddenWindow> = Vec::new();
    let mut seen_processes = std::collections::HashSet::new();
    for candidate in candidates {
        if !seen_processes.insert(candidate.process_id) {
            continue;
        }
        let visual = crate::shell::icons::visual_for_process(candidate.process_id);
        windows.push(HiddenWindow {
            id: candidate.hwnd,
            name: prettify_process(&visual.process_name),
            title: candidate.title,
            process_name: visual.process_name,
            icon: visual.icon,
        });
    }
    windows.sort_by_key(|entry| entry.title.to_lowercase());
    Ok(windows)
}

/// The tray tooltip users recognize is the program's own name: tidy the
/// executable stem into one (OneDrive, LG Hub, PowerToys FancyZones...).
pub(crate) fn prettify_process(stem: &str) -> String {
    let separators: [char; 3] = ['.', '_', '-'];
    let mut name = stem
        .replace(separators, " ")
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ");
    if name.is_empty() {
        name = stem.to_string();
    }
    name
}

/// Reveals a background window: restores it if minimized, shows it, and
/// brings it to the foreground.
pub fn reveal(id: isize) -> Result<(), String> {
    let hwnd = HWND(id as *mut _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if SetForegroundWindow(hwnd).as_bool() {
            Ok(())
        } else {
            Err("Windows rejected the foreground activation request".to_owned())
        }
    }
}
