//! Liquid glass surface commands: hit rects, menu glass placement, warm-up
//! and the webview's reconciliation sweep.

use crate::renderer::RendererState;
use glass_ui::{GlassMaterialOverride, MetaballPush};

#[derive(serde::Deserialize, Debug)]
pub struct MenuGlassRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Replaces the taskbar's interactive-element rects (overlay-relative physical
/// pixels). The hit-test loop keeps the overlay click-through everywhere
/// except these and the open menus', so clicks on empty pill areas reach the
/// windows behind.
#[tauri::command]
pub fn set_ui_hit_rects(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    rects: Vec<MenuGlassRect>,
) -> Result<(), String> {
    let overlay = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    let mut host = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
            windows::Win32::Foundation::HWND(overlay as *mut _),
            &mut host,
        );
    }
    state.set_ui_hit_rects(
        rects
            .iter()
            .map(|rect| {
                [
                    host.left + rect.x as i32,
                    host.top + rect.y as i32,
                    host.left + (rect.x + rect.width) as i32,
                    host.top + (rect.y + rect.height) as i32,
                ]
            })
            .collect(),
    );
    Ok(())
}

/// Pre-creates warm menu glass surfaces into the recycle pool so the next
/// grouped panel opens instantly. Called on trigger hover, buying the
/// capture-session warmup time before the click. `local` pre-warms
/// in-app (local-feed) surfaces instead of the global ones — the held
/// glass thumbs' pool.
#[tauri::command]
pub fn prewarm_menu_glass(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    count: usize,
    local: Option<bool>,
) -> Result<(), String> {
    if local.unwrap_or(false) {
        eprintln!(
            "[glass] prewarm local host={:#x} count={count}",
            window.hwnd().map(|hwnd| hwnd.0 as isize).unwrap_or(0)
        );
    }
    state.prewarm_glasses(
        windows::Win32::Foundation::HWND(window.hwnd().map_err(|e| e.to_string())?.0 as *mut _),
        count,
        local.unwrap_or(false),
    )
}

/// Renders (or removes) the native liquid glass surface behind a menu. The
/// rect is in physical pixels relative to the overlay's top-left corner.
/// `local` feeds the surface from the host window's own capture instead of
/// the shared monitor capture (in-app glass; Glassbar's own menus are
/// always global).
///
/// `shape_kind`/`shape_exponent` select the surface's own preset: 0 (or
/// absent) is the rounded rect of `rect_radius`, 1 draws the superellipse
/// (squircle) of `shape_exponent`.
///
/// `metaball` turns the surface into a metaball container (see glass-ui's
/// command surface for the field semantics).
///
/// `material` pins the surface's glass material to per-surface values (the
/// web UI's `material` prop; the held glass thumbs use it).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn position_menu_glass(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    key: String,
    rect: Option<MenuGlassRect>,
    rect_radius: Option<f64>,
    instant: Option<bool>,
    glass_fade: Option<bool>,
    group: Option<String>,
    spring: Option<bool>,
    local: Option<bool>,
    shape_kind: Option<f64>,
    shape_exponent: Option<f64>,
    metaball: Option<bool>,
    metaball_radius: Option<f64>,
    metaball_balls: Option<Vec<[f64; 6]>>,
    metaball_tension: Option<f64>,
    material: Option<GlassMaterialOverride>,
) -> Result<(), String> {
    let rect = rect.map(|r| [r.x as f32, r.y as f32, r.width as f32, r.height as f32]);
    let radius = rect_radius.unwrap_or(20.0) as f32;
    let shape = match shape_kind {
        Some(1.0) => Some(liquid_glass::GlassShape::Superellipse {
            exponent: shape_exponent.unwrap_or(5.0) as f32,
        }),
        _ => None,
    };
    let metaball = metaball.unwrap_or(false).then(|| MetaballPush {
        ball_radius: metaball_radius.unwrap_or(24.0) as f32,
        balls: metaball_balls.unwrap_or_default(),
        tension: metaball_tension.unwrap_or(2.0) as f32,
    });
    // Trace the in-app (local) thumbs only: the global menu traffic is far
    // too busy to log.
    if local.unwrap_or(false) {
        eprintln!(
            "[glass] position host={:#x} key={key} rect={rect:?} radius={radius} material={}",
            window.hwnd().map(|hwnd| hwnd.0 as isize).unwrap_or(0),
            material.is_some()
        );
    }
    state.position_menu_glass(
        windows::Win32::Foundation::HWND(window.hwnd().map_err(|e| e.to_string())?.0 as *mut _),
        &key,
        rect,
        radius,
        instant.unwrap_or(false),
        glass_fade.unwrap_or(false),
        group.as_deref(),
        spring.unwrap_or(false),
        local.unwrap_or(false),
        shape,
        metaball,
        material,
    )
}

/// Animates (or re-states) a menu glass's CSS-style transform: the drawn
/// shape transitions into `animation`'s target through its cubic-bezier
/// curve. The held glass thumbs' grow/settle tweens ride on this.
#[tauri::command]
pub fn animate_menu_glass(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    key: String,
    animation: glass_ui::GlassTransformAnimation,
) -> Result<(), String> {
    eprintln!("[glass] animate key={key}");
    state.animate_menu_glass(
        windows::Win32::Foundation::HWND(window.hwnd().map_err(|e| e.to_string())?.0 as *mut _),
        &key,
        animation,
    )
}

/// The webview's reconciliation sweep: reports the menu-glass keys it
/// currently tracks. Any native surface OF THIS WINDOW whose key is
/// missing gets closed — a hard guarantee that a lost close push can never
/// leave a glass (e.g. a wifi panel) visible forever after a settings
/// change. Scoped to the calling window so several webview windows
/// sharing one manager never despawn each other's glass.
#[tauri::command]
pub fn sync_menu_glasses(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    keys: Vec<String>,
) {
    let host = window.hwnd().map(|hwnd| hwnd.0 as isize).unwrap_or(0);
    state.sync_menu_glasses(&keys, host);
}
