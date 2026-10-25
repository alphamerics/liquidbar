//! Taskbar pill commands: the AppBar reservation toggle, the measured content
//! width (with self-heal and the resize spring), plus misc small commands.

use crate::renderer::{RendererState, SpringTarget};
use crate::settings::GlassbarSettings;
use crate::shell;
use tauri::Manager;

/// Toggles the AppBar registration that stops maximized/fullscreen apps from
/// covering the taskbar strip. When disabled, apps may cover the pill (the
/// pill stays topmost and floats above them).
#[tauri::command]
pub fn set_reserve_space(
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<(), String> {
    state.lock().expect("settings lock poisoned").reserve_space = enabled;
    renderer.set_autohide(!enabled);
    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    if enabled {
        shell::register_appbar(hwnd, renderer.taskbar_content_width() as i32)
    } else {
        shell::unregister_appbar(hwnd);
        Ok(())
    }
}

/// The webview reports the pill's measured content width (physical pixels)
/// whenever it changes: the pill grows to fit its content and shrinks back
/// when it needs less, clamped to a sane floor and below the monitor width
/// so it can never overflow the screen. A real on-screen resize also plays
/// the squash & stretch spring on the glass and the DOM content.
#[tauri::command]
pub fn set_taskbar_content_width(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
    desired_width: f64,
) {
    let previous = renderer.set_taskbar_content_width(desired_width);
    let width = shell::taskbar_width_px(desired_width as i32);
    let unchanged = (previous - desired_width).abs() < 1.0;
    // Self-heal: re-place a window whose actual size drifted away from what
    // the LAST reposition actually landed on (external interference, snap
    // re-adjustments). Comparing against the REQUESTED width instead would
    // fight the AppBar's own snap in an endless reposition loop — the
    // visible "twitching while slowly growing" bug.
    let drifted = unchanged
        && renderer.applied_taskbar_width() != 0
        && app
            .get_webview_window("main")
            .and_then(|window| window.inner_size().ok())
            .map(|size| (size.width as i32 - renderer.applied_taskbar_width()).abs() >= 10)
            .unwrap_or(false);
    if unchanged && !drifted {
        return;
    }
    let (squash, bounce, duration_ms) = {
        let guard = state.lock().expect("settings lock poisoned");
        (
            guard.resize_squash,
            guard.resize_bounce,
            guard.resize_duration,
        )
    };
    // Springs key on the effective (clamped) width: only a real on-screen
    // resize should squash the pill. Boot-settle suppresses the spring
    // entirely (None) while the webview is still measuring content.
    let previous_effective = renderer.set_effective_taskbar_width(width as f64);
    let resize_anim = if !unchanged && (previous_effective - width as f64).abs() >= 1.0 {
        renderer.trigger_pill_resize_spring(
            SpringTarget::Pill,
            previous_effective,
            width as f64,
            squash,
            bounce,
            duration_ms,
        )
    } else {
        None
    };
    let reserve_space = app
        .state::<std::sync::Mutex<GlassbarSettings>>()
        .lock()
        .expect("settings lock poisoned")
        .reserve_space;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let result = if reserve_space {
        // A width change must NOT unregister the AppBar: ABM_REMOVE lets the
        // real taskbar pop up and the shell then shoves the bar upward
        // permanently. ABM_SETPOS alone re-snaps the new strip.
        shell::reposition_appbar(hwnd.0 as isize, width)
    } else {
        // No reservation: re-center the floating pill without touching its
        // bottom edge.
        shell::reposition_unregistered(hwnd.0 as isize, width)
    };
    if let Err(error) = result {
        eprintln!("Could not apply the taskbar width {width}px: {error}");
        return;
    }
    // Record what the window ACTUALLY became (the AppBar snap may adjust the
    // requested rect) — this is the baseline the next drift check compares
    // against, so a persistent snap can't turn into a heal loop.
    let landed = app
        .get_webview_window("main")
        .and_then(|window| window.inner_size().ok())
        .map(|size| size.width as i32)
        .unwrap_or(width);
    let previous_landed = renderer.set_applied_taskbar_width(landed);
    if drifted || previous_landed != landed {
        eprintln!(
            "[taskbar-width] clamped={width} landed={landed} previous_landed={previous_landed} drifted={drifted}"
        );
    }
    if let Some(ratio) = resize_anim {
        use tauri::Emitter;
        let _ = app.emit(
            "pill-resize-anim",
            PillResizeAnim {
                from_width: previous_effective,
                to_width: width as f64,
                ratio,
                squash,
                bounce,
                duration_ms,
            },
        );
    }
}

/// Payload of the `pill-resize-anim` event: the resize spring's initial
/// conditions, mirrored by the DOM content spring in Taskbar.tsx so the
/// webview content and the native glass squash as one.
#[derive(serde::Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PillResizeAnim {
    pub from_width: f64,
    pub to_width: f64,
    pub ratio: f64,
    pub squash: f64,
    pub bounce: f64,
    pub duration_ms: f64,
}

/// Quits Glassbar: the Windows taskbar (and its original AppBar state) is
/// restored first — the exit handler does this too, but restoring before
/// `app.exit` keeps the taskbar back even if teardown gets interrupted.
#[tauri::command]
pub fn exit_glassbar(app: tauri::AppHandle) {
    crate::shell::restore_taskbar();
    app.exit(0);
}

#[tauri::command]
pub fn log_message(message: String) {
    eprintln!("[ui] {message}");
}
