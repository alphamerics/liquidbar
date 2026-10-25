//! Core shell commands: the taskbar snapshot, window activation/closing and
//! the live DWM-thumbnail preview.

use crate::shell::{self, ShellSnapshot, ShellState};

#[tauri::command]
pub fn shell_snapshot(app: tauri::AppHandle, state: tauri::State<'_, ShellState>) -> ShellSnapshot {
    shell::snapshot(&app, &state)
}

/// The focused app's active keyboard layout (the "input language").
#[tauri::command]
pub fn keyboard_layout() -> crate::shell::KeyboardLayout {
    crate::shell::keyboard::snapshot()
}

#[tauri::command]
pub fn activate_window(id: isize, minimize_toggle: Option<bool>) -> Result<(), String> {
    shell::activate(id, minimize_toggle.unwrap_or(false))
}

#[tauri::command]
pub fn close_app(id: isize) -> Result<(), String> {
    shell::close_window(id)
}

#[derive(serde::Deserialize, Debug)]
pub struct PreviewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: Option<f64>,
}

/// Shows (or hides) the live DWM-thumbnail preview of one app window. The
/// rect is in physical pixels relative to the overlay's top-left corner; the
/// radius is the thumbnail's corner radius in physical pixels.
#[tauri::command]
pub fn set_window_preview(
    window: tauri::WebviewWindow,
    id: Option<isize>,
    rect: Option<PreviewRect>,
) -> Result<(), String> {
    let overlay = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    shell::preview::set_preview(
        &window,
        overlay,
        id,
        rect.as_ref()
            .map(|r| [r.x as i32, r.y as i32, r.width as i32, r.height as i32]),
        rect.as_ref().and_then(|r| r.radius).unwrap_or(13.0) as i32,
    )
}

