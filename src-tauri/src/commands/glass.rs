//! Liquid glass surface commands: hit rects, menu glass placement, warm-up
//! and the webview's reconciliation sweep.

use crate::renderer::RendererState;

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
/// capture-session warmup time before the click.
#[tauri::command]
pub fn prewarm_menu_glass(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RendererState>,
    count: usize,
) -> Result<(), String> {
    state.prewarm_glasses(
        windows::Win32::Foundation::HWND(window.hwnd().map_err(|e| e.to_string())?.0 as *mut _),
        count,
    )
}

/// Renders (or removes) the native liquid glass surface behind a menu. The
/// rect is in physical pixels relative to the overlay's top-left corner.
/// `local` feeds the surface from the host window's own capture instead of
/// the shared monitor capture (in-app glass; Glassbar's own menus are
/// always global).
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
) -> Result<(), String> {
    let rect = rect.map(|r| [r.x as f32, r.y as f32, r.width as f32, r.height as f32]);
    let radius = rect_radius.unwrap_or(20.0) as f32;
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
    let host = window
        .hwnd()
        .map(|hwnd| hwnd.0 as isize)
        .unwrap_or(0);
    state.sync_menu_glasses(&keys, host);
}
