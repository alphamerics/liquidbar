//! Pinned app commands: pin/unpin running windows and launch closed pins.

use crate::shell::{self, ShellState};

/// Pins a running window: stores its title, icon and executable path so the
/// taskbar keeps showing it (and can launch it) while it is closed.
#[tauri::command]
pub fn pin_app(app: tauri::AppHandle, state: tauri::State<'_, ShellState>, id: isize) {
    let Some(entry) = shell::pinned_entry_for_window(id) else {
        return;
    };
    let mut pinned = state.pinned.lock().expect("pinned lock poisoned");
    if !pinned
        .iter()
        .any(|existing| existing.process_name == entry.process_name)
    {
        pinned.push(entry);
    }
    shell::save_pinned(&app, &pinned);
}

#[tauri::command]
pub fn unpin_app(app: tauri::AppHandle, state: tauri::State<'_, ShellState>, process_name: String) {
    let mut pinned = state.pinned.lock().expect("pinned lock poisoned");
    pinned.retain(|name| name.process_name != process_name);
    shell::save_pinned(&app, &pinned);
}

/// Launches a pinned app's stored executable while it is closed.
#[tauri::command]
pub fn launch_pinned(executable: String) -> Result<(), String> {
    shell::launch_executable(&executable)
}
