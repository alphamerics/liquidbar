//! The keyboard's current input language: the foreground thread's active
//! keyboard layout (HKL), the same value Windows' own language indicator
//! shows in the corner of the taskbar.
//!
//! Layout is per-thread in Windows: switching layout (Win+Space / Alt+Shift)
//! or switching to an app pinned to another layout both change what the
//! foreground window would receive, so the "current mode" is read from the
//! focused window's thread, with fallbacks for the no-focus case.

use serde::Serialize;
use windows::Win32::Globalization::{
    GetLocaleInfoW, GetUserDefaultUILanguage, LOCALE_SENGLISHDISPLAYNAME,
    LOCALE_SISO639LANGNAME2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// One keyboard-layout snapshot for the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardLayout {
    /// Short language tag like "ENG", rendered as the gadget's indicator.
    pub short: String,
    /// Human-readable display name like "English (United States)", for the
    /// gadget's tooltip.
    pub name: String,
}

/// Reads the keyboard layout of the foreground window's thread.
pub fn snapshot() -> KeyboardLayout {
    // Preferred: the focused window's thread carries the layout its app is
    // currently typing with.
    let foreground = unsafe { GetForegroundWindow() };
    let hkl = if !foreground.0.is_null() {
        let thread = unsafe { GetWindowThreadProcessId(foreground, None) };
        if thread != 0 {
            unsafe { GetKeyboardLayout(thread) }
        } else {
            unsafe { GetKeyboardLayout(0) }
        }
    } else {
        unsafe { GetKeyboardLayout(0) }
    };
    let mut layout = keyboard_layout_from_hkl(hkl);
    if layout.short.is_empty() {
        // No layout on the foreground thread: the user default is the next
        // best answer.
        layout = keyboard_layout_from_langid(unsafe { GetUserDefaultUILanguage() } as u32);
    }
    layout
}

/// Extracts the input language from an HKL (its low word is the LANGID) and
/// resolves it to the display strings via the locale database.
fn keyboard_layout_from_hkl(hkl: windows::Win32::UI::Input::KeyboardAndMouse::HKL) -> KeyboardLayout {
    keyboard_layout_from_langid((hkl.0 as usize & 0xFFFF) as u32)
}

/// Resolves a language identifier (LANGID) to the indicator strings.
fn keyboard_layout_from_langid(langid: u32) -> KeyboardLayout {
    // MAKELCID(langid, SORT_DEFAULT) == langid (sort id 0 lives in the high
    // word, which stays zero).
    let lcid = langid & 0xFFFF;
    let mut short = locale_string(lcid, LOCALE_SISO639LANGNAME2).to_uppercase();
    if short.is_empty() {
        short = "ENG".to_owned();
    }
    KeyboardLayout {
        short,
        name: locale_string(lcid, LOCALE_SENGLISHDISPLAYNAME),
    }
}

/// One locale string property (NUL-terminated) for an LCID.
fn locale_string(lcid: u32, info: u32) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetLocaleInfoW(lcid, info, Some(&mut buffer)) };
    if len <= 0 {
        return String::new();
    }
    let len = (len as usize).min(buffer.len());
    // The returned count includes the terminator.
    String::from_utf16_lossy(&buffer[..len - 1])
}
