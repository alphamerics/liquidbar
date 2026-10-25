//! Click-through hit-testing, in one place.
//!
//! The overlay is one fixed-size window taller than the taskbar pill (menus
//! grow into the spare space without a flicker-inducing resize), so most of
//! its area must let clicks fall through to the windows behind. Each tick
//! measures where the cursor sits relative to the overlay and its
//! interactive elements, then applies the result by toggling the window's
//! `WS_EX_TRANSPARENT` ex-style.

use windows::Win32::{
    Foundation::{POINT, RECT},
    UI::WindowsAndMessaging::{GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_TRANSPARENT},
};

use crate::shell;

/// Whether the cursor is inside one `[left, top, right, bottom]` rect
/// (right/bottom exclusive, so adjacent rects never double-hit).
fn in_rect(cursor: &POINT, hit: &[i32; 4]) -> bool {
    cursor.x >= hit[0] && cursor.x < hit[2] && cursor.y >= hit[1] && cursor.y < hit[3]
}

/// The cursor's relationship to the overlay this tick.
pub struct CursorHover {
    /// The cursor is over an interactive element: the taskbar's buttons
    /// (pushed by the webview) or an open menu/preview (tracked by the
    /// renderer). Clicks land on the overlay only here — everywhere else
    /// stays click-through, so empty pill areas and the oversized body never
    /// block the windows behind.
    pub interactive: bool,
    /// The cursor is over the taskbar pill's bottom strip, whether or not it
    /// is over an interactive element: while the taskbar floats, that alone
    /// holds the bar up (the reveal trigger's pill term).
    pub pill_hover: bool,
}

impl CursorHover {
    /// Measures the cursor against the overlay's rect, the renderer's hit
    /// rects and the pill strip.
    pub fn measure(cursor: POINT, rect: RECT, hit_rects: &[[i32; 4]]) -> Self {
        let inside = cursor.x >= rect.left
            && cursor.x < rect.right
            && cursor.y >= rect.top
            && cursor.y < rect.bottom;
        let interactive = inside && hit_rects.iter().any(|hit| in_rect(&cursor, hit));
        let pill_hover = inside && cursor.y >= shell::overlay_bottom_y() - shell::TASKBAR_HEIGHT_PX;
        Self {
            interactive,
            pill_hover,
        }
    }
}

/// Applies the hit-test decision to the window: clearing
/// `WS_EX_TRANSPARENT` makes the overlay take clicks, setting it lets them
/// fall through. The style write is skipped when the style already matches.
pub fn apply(hwnd: windows::Win32::Foundation::HWND, interactive: bool) {
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let updated = if interactive {
            style & !WS_EX_TRANSPARENT.0
        } else {
            style | WS_EX_TRANSPARENT.0
        };
        if updated != style {
            SetWindowLongW(hwnd, GWL_EXSTYLE, updated as i32);
        }
    }
}
