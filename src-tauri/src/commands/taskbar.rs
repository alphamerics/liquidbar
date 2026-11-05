//! Taskbar pill commands: the AppBar reservation toggle, the measured content
//! width (with self-heal and the resize spring), plus misc small commands.

use crate::renderer::{RendererState, SpringTarget};
use crate::settings::GlassbarSettings;
use crate::shell;
use tauri::{Emitter, Manager};

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
/// the squash & stretch spring on the glass and the DOM content — unless
/// `animate` is false, which marks the report as a consequence of a pill
/// height change (the rescaled content, measured during the height's flush
/// window) that must never spawn a width spring.
#[tauri::command]
pub fn set_taskbar_content_width(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
    desired_width: f64,
    animate: Option<bool>,
) {
    let animate = animate.unwrap_or(true);
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
    // entirely (None) while the webview is still measuring content, and so
    // does a non-animated report (a pill-height consequence) — springing
    // on those is the "pill keeps bouncing after the drag ended" artifact.
    let previous_effective = renderer.set_effective_taskbar_width(width as f64);
    let resize_anim = if !unchanged && (previous_effective - width as f64).abs() >= 1.0 && animate {
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
    // Stage the glass geometry for the new width BEFORE the reposition (see
    // set_taskbar_height): the canvas anchor rides the resize's own event
    // pass and the drawn shape retargets immediately — a constant-buffer
    // update inside the fixed canvas, so width changes never flash the glass.
    renderer.stage_taskbar_pill_geometry(width as f32, shell::taskbar_height_px() as f32);
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

/// The pill height's sane bounds (physical pixels): tall enough to stay
/// usable, short enough to never crowd the screen.
pub const TASKBAR_HEIGHT_MIN_PX: f64 = 48.0;
pub const TASKBAR_HEIGHT_MAX_PX: f64 = 160.0;

/// Resizes the taskbar pill live (the webview's top-edge resize drag,
/// macOS-dock style): updates the persisted setting, the native shell
/// geometry (AppBar strip, overlay window) and the glass pill's anchor.
///
/// The drag also passes the proportionally scaled pill width, so height and
/// width apply in ONE reposition — separate width reports would land a
/// frame later, leaving the DOM visibly wider than the window mid-drag.
///
/// Called for every drag step, so the per-step work stays minimal: with
/// `commit = false` (mid-drag) only the geometry moves — the settings JSON
/// write and the broadcast round-trip happen once on the final
/// `commit = true` call, when the drag ends.
#[tauri::command]
pub fn set_taskbar_height(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
    desired_height: f64,
    desired_width: Option<f64>,
    commit: bool,
) -> Result<(), String> {
    let height = desired_height
        .round()
        .clamp(TASKBAR_HEIGHT_MIN_PX, TASKBAR_HEIGHT_MAX_PX) as i32;
    let width = desired_width.map(|w| shell::taskbar_width_px(w.round() as i32));
    let unchanged = shell::taskbar_height_px() == height
        && width.map_or(true, |w| (renderer.applied_taskbar_width() - w).abs() < 1);
    if unchanged && !commit {
        return Ok(());
    }
    if !unchanged {
        // The shell geometry derives from the atomic, so it must move first.
        shell::set_taskbar_height_px(height);
        // Keep the width bookkeeping aligned, so a later width report can
        // never fight the value this reposition just landed on.
        if let (Some(w), Some(raw)) = (width, desired_width) {
            renderer.set_taskbar_content_width(raw);
            renderer.set_effective_taskbar_width(w as f64);
        }
        let reserve_space = {
            let mut guard = state.lock().expect("settings lock poisoned");
            guard.taskbar_height = height as f64;
            guard.reserve_space
        };
        let Some(window) = app.get_webview_window("main") else {
            return Ok(());
        };
        let Ok(hwnd) = window.hwnd() else {
            return Ok(());
        };
        let width =
            shell::taskbar_width_px(width.unwrap_or(renderer.taskbar_content_width() as i32));
        // Stage the new glass geometry BEFORE the window resize: the canvas
        // anchor rides the resize's own event pass, and the drawn shape is
        // retargeted right away — a constant-buffer update inside the glass's
        // fixed canvas (no window ops, no swap-chain resize, no flash frame).
        renderer.stage_taskbar_pill_geometry(width as f32, height as f32);
        // Mid-drag steps take the live fast path: a plain SetWindowPos, no
        // shell-synchronizing AppBar round-trip (that per-step latency is
        // the drag stutter). The commit below re-snaps the reservation.
        let result = if !commit {
            shell::reposition_live(hwnd.0 as isize, width)
        } else if reserve_space {
            shell::reposition_appbar(hwnd.0 as isize, width)
        } else {
            shell::reposition_unregistered(hwnd.0 as isize, width)
        };
        result?;
        // The commit path re-asserts the full geometry after the reposition
        // (the AppBar snap may have adjusted the rect). Live steps need
        // nothing here: the shape already tracks the new size and the
        // resize's event pass synced the canvas position.
        if commit {
            if let Err(error) = renderer.set_taskbar_pill_geometry(width as f32, height as f32) {
                eprintln!("Could not retarget the glass pill: {error}");
            }
        }
        // Record what the window ACTUALLY landed on (the AppBar snap may
        // adjust the rect) — the baseline the width pipeline's drift check
        // compares against, so this resize can't turn into a heal loop.
        let landed = window
            .inner_size()
            .map(|size| size.width as i32)
            .unwrap_or(width);
        renderer.set_applied_taskbar_width(landed);
    }
    if commit {
        let settings = state.lock().expect("settings lock poisoned").clone();
        shell::save_settings(&app, &settings);
        app.emit("settings-changed", crate::settings::full_json(&settings))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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
