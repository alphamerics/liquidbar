//! Settings commands: read, apply live and reset the user settings, and the
//! settings window lifecycle.

use crate::renderer::RendererState;
use crate::settings::{shader_params_from, GlassbarSettings};
use crate::shell;
use tauri::{Emitter, Manager};

#[tauri::command]
pub fn get_settings(
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
) -> serde_json::Value {
    let settings = state.lock().expect("settings lock poisoned").clone();
    crate::settings::full_json(&settings)
}

/// Applies every saved setting to the freshly created renderer at startup:
/// the glass material, the pill's corner radius, and the auto-hide mode.
pub fn apply_startup_settings(app: &tauri::App, renderer: &RendererState) {
    let settings = app
        .state::<std::sync::Mutex<GlassbarSettings>>()
        .lock()
        .expect("settings lock poisoned")
        .clone();
    if let Err(error) = renderer.apply_glass_settings(shader_params_from(&settings)) {
        eprintln!("Could not apply startup glass settings: {error}");
    }
    if let Err(error) = renderer.set_taskbar_corner_radius(settings.pill_radius as f32) {
        eprintln!("Could not apply the startup corner radius: {error}");
    }
    renderer.set_autohide(!settings.reserve_space);
    shell::set_taskbar_hidden(settings.hide_taskbar);
    // Always re-sync the Run key with the saved setting: it makes the
    // on-by-default registration happen on first run, keeps the key in
    // step with a hand-edited settings.json, and repairs a value pointing
    // at a moved executable.
    shell::set_launch_on_startup(settings.launch_on_startup);
    eprintln!(
        "[boot] startup settings applied (pill_radius={}, autohide={}, hide_taskbar={})",
        settings.pill_radius, !settings.reserve_space, settings.hide_taskbar
    );
}

/// Applies settings live: the glass material, the pill's corner radius,
/// auto-hide and the AppBar (re)registration. Shared by set_settings and
/// reset_settings so both paths stay identical.
fn apply_settings(
    app: &tauri::AppHandle,
    state: &std::sync::Mutex<GlassbarSettings>,
    renderer: &RendererState,
    settings: GlassbarSettings,
) -> Result<(), String> {
    let (previous_reserve, previous_radius, previous_hide_taskbar, previous_launch_on_startup) = {
        let guard = state.lock().expect("settings lock poisoned");
        (
            guard.reserve_space,
            guard.pill_radius,
            guard.hide_taskbar,
            guard.launch_on_startup,
        )
    };
    let reserve_changed = previous_reserve != settings.reserve_space;
    let radius_changed = (previous_radius - settings.pill_radius).abs() >= 0.5;
    let hide_taskbar_changed = previous_hide_taskbar != settings.hide_taskbar;
    let launch_on_startup_changed = previous_launch_on_startup != settings.launch_on_startup;
    *state.lock().expect("settings lock poisoned") = settings.clone();
    shell::save_settings(app, &settings);
    // The resize-spring parameters are cached on the renderer so any spring
    // trigger (pill resize, preview re-anchor) uses the live values.
    renderer.set_resize_anim(
        settings.resize_squash,
        settings.resize_bounce,
        settings.resize_duration,
    );
    if let Err(error) = renderer.apply_glass_settings(shader_params_from(&settings)) {
        eprintln!("Could not apply glass settings: {error}");
    }
    if radius_changed {
        if let Err(error) = renderer.set_taskbar_corner_radius(settings.pill_radius as f32) {
            eprintln!("Could not apply the corner radius: {error}");
        }
    }
    // The Windows taskbar is hidden (or restored) directly, independent of
    // the AppBar registration.
    if hide_taskbar_changed {
        shell::set_taskbar_hidden(settings.hide_taskbar);
    }
    // The Run key only needs touching when the setting itself changed; a
    // reset that restores the default still fires here because the
    // previous value was already read.
    if launch_on_startup_changed {
        shell::set_launch_on_startup(settings.launch_on_startup);
    }
    // The AppBar registration is what actually sizes and places the overlay.
    // A reservation toggle needs the full unregister/register churn. The
    // pill's width is no longer a setting: the webview reports its measured
    // content width and the overlay follows it (see set_taskbar_content_width).
    if reserve_changed {
        renderer.set_autohide(!settings.reserve_space);
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(hwnd) = window.hwnd() {
                shell::unregister_appbar(hwnd.0 as isize);
                if settings.reserve_space {
                    shell::register_appbar(
                        hwnd.0 as isize,
                        renderer.taskbar_content_width() as i32,
                    )?;
                } else {
                    // Re-place the floating pill at the current width.
                    shell::reposition_unregistered(
                        hwnd.0 as isize,
                        renderer.taskbar_content_width() as i32,
                    )?;
                }
            }
        }
    }
    let _ = app.emit("settings-changed", crate::settings::full_json(&settings));
    Ok(())
}

#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
    patch: serde_json::Value,
) -> Result<(), String> {
    // Merge the patch into the authoritative native state field by field.
    // Every window (settings, taskbar, control center) writes its own fields
    // here instead of a full snapshot, so a write racing another window's
    // write can no longer revert fields it did not touch.
    let current = state.lock().expect("settings lock poisoned").clone();
    let mut merged =
        serde_json::to_value(&current).map_err(|error| error.to_string())?;
    if let (Some(fields), Some(patch_fields)) = (merged.as_object_mut(), patch.as_object()) {
        for (key, value) in patch_fields {
            fields.insert(key.clone(), value.clone());
        }
        // The material fields are serde-skipped (they live in the global
        // material file), so the serialized current state omits them.
        // Re-inject the current values so a patch that touches, say, icon
        // size cannot zero out the material — unless the patch explicitly
        // sets a material key (the settings UI's glass edits).
        let material = crate::settings::material_from(&current);
        if let Ok(material_json) = serde_json::to_value(&material) {
            if let Some(material_fields) = material_json.as_object() {
                for (key, value) in material_fields {
                    fields.entry(key.clone()).or_insert(value.clone());
                }
            }
        }
        // Any explicit layout write — including a removal — proves the
        // layout is user-customized: bump its version so the legacy
        // migration can never mistake it for the uncustomized default.
        if patch_fields.contains_key("ccLayout") {
            fields.insert(
                "ccLayoutVersion".to_string(),
                serde_json::Value::from(crate::settings::CC_LAYOUT_VERSION_CURRENT),
            );
        }
    }
    let settings: GlassbarSettings =
        serde_json::from_value(merged).map_err(|error| error.to_string())?;
    eprintln!(
        "[settings] patch keys: {:?} -> cc toggles {:?}, layout {} items",
        patch.as_object().map(|fields| fields.keys().cloned().collect::<Vec<_>>()),
        settings.cc_toggles,
        settings.cc_layout.len(),
    );
    apply_settings(&app, &state, &renderer, settings)
}

/// Restores every setting to its default and applies it live. Returns the
/// defaults so the settings UI can refresh its controls.
#[tauri::command]
pub fn reset_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<GlassbarSettings>>,
    renderer: tauri::State<'_, RendererState>,
) -> Result<serde_json::Value, String> {
    let settings = GlassbarSettings::default();
    // Return the value the frontend reads: the serde-skipped material
    // fields must ride along (see settings::full_json) or the Liquid
    // Glass pane renders undefined sliders/colors and crashes.
    let payload = crate::settings::full_json(&settings);
    apply_settings(&app, &state, &renderer, settings)?;
    Ok(payload)
}

/// Opens (or focuses) the framed, monitor-centered settings window.
///
/// Must stay an `async fn`: synchronous commands run on the main thread, and
/// building a webview window there blocks the event loop's message pump, so
/// WebView2 never gets to attach its controller and the window stays blank.
#[tauri::command]
pub async fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!(
        "[settings] open requested on thread {:?}",
        std::thread::current().id()
    );
    if let Some(existing) = app.get_webview_window("settings") {
        eprintln!("[settings] reusing existing settings window");
        let _ = existing.unminimize();
        let _ = existing.show();
        let _ = existing.set_focus();
        app.state::<RendererState>()
            .set_settings_open(true);
        return Ok(());
    }
    // In dev the app runs from the vite server; App() paths can resolve
    // inconsistently for secondary windows, so point straight at the server.
    // Both URLs target the #/settings route so App() renders the settings
    // page immediately, without waiting for window-label detection.
    #[cfg(debug_assertions)]
    let builder_url: tauri::WebviewUrl = tauri::WebviewUrl::External(
        "http://localhost:1420/#/settings"
            .parse::<tauri::Url>()
            .map_err(|error| error.to_string())?,
    );
    #[cfg(not(debug_assertions))]
    let builder_url = tauri::WebviewUrl::App("index.html".into());
    eprintln!(
        "[settings] building window on thread {:?}",
        std::thread::current().id()
    );
    let window = tauri::WebviewWindowBuilder::new(&app, "settings", builder_url)
        .title("Glassbar Settings")
        .skip_taskbar(true)
        .inner_size(680.0, 620.0)
        .center()
        .resizable(true)
        // Keep wry's default feature disables and turn off Chromium's
        // native window occlusion: it suspends the compositor while the
        // window is covered by other windows, blanking the DOM. The forced
        // 1x scale factor keeps the web UI identical on every machine —
        // Windows' DPI scaling otherwise resizes the DOM (CSS px = physical
        // px everywhere else in the app).
        .additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion --force-device-scale-factor=1",
        )
        .build()
        .map_err(|error| format!("Could not open the settings window: {error}"))?;
    eprintln!(
        "[settings] build() returned on thread {:?}",
        std::thread::current().id()
    );
    // While the settings window is open the taskbar must not auto-hide: the
    // pill stays put until the window is closed (or otherwise destroyed).
    app.state::<RendererState>()
        .set_settings_open(true);
    {
        let handle = app.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                handle
                    .state::<RendererState>()
                    .set_settings_open(false);
            }
        });
    }
    match window.url() {
        Ok(url) => eprintln!("[settings] built; url={url}"),
        Err(error) => eprintln!("[settings] built; url error: {error}"),
    }
    match window.hwnd() {
        Ok(hwnd) => eprintln!("[settings] built; hwnd={}", hwnd.0 as isize),
        Err(error) => eprintln!("[settings] built; hwnd error: {error}"),
    }
    #[cfg(debug_assertions)]
    {
        // Make the page's own devtools visible so console output is viewable.
        window.open_devtools();
    }
    // No production hash navigation: reading window.url() right after
    // build() returns `about:blank` (the app URL navigation has not started
    // yet), so navigating to `#/settings` would land on about:blank â€” a
    // blank page. The app detects this window by its label instead
    // (getCurrentWindow().label === "settings"), which works on the plain
    // app URL.
    Ok(())
}
