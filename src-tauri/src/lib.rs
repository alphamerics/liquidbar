//! Glassbar's native side. This crate is the Tauri backend: the GPU liquid
//! glass renderer (`renderer`), the Windows shell integration (`shell`), the
//! shared user settings (`settings`), the overlay supervision loop
//! (`hit_test_loop`) and the Tauri command surface (`commands`, one module
//! per feature area). This file only wires the pieces together.

mod commands;
mod hit_test_loop;
mod renderer;
mod shell;
mod settings;
mod wheel_volume;

pub use settings::GlassbarSettings;

use commands::settings::apply_startup_settings;
use hit_test_loop::spawn_hit_test_loop;
use renderer::RendererState;
use shell::ShellState;
use std::sync::Mutex;
use tauri::{App, AppHandle, Manager, WebviewWindow};
use windows::Win32::Foundation::HWND;

/// Entry point of the `tray_probe` diagnostic binary: enumerates Explorer's
/// tray service and reports the stored registrations. Read-only by default;
/// `--dispatch <key>` forwards one real context-menu event instead.
pub fn tray_probe_main() {
    let mut args = std::env::args().skip(1);
    let report = match (args.next().as_deref(), args.next().as_deref()) {
        (Some("--dispatch"), Some(key)) => shell::tray_probe_dispatch(key),
        _ => shell::tray_probe_report(),
    };
    println!("{report}");
}

/// Hydrates the managed state from disk before anything reads it: the pinned
/// apps, the shared settings, and the Do-not-disturb reconciliation.
fn apply_startup_state(app: &App) {
    *app.state::<ShellState>()
        .pinned
        .lock()
        .expect("pinned lock poisoned") = shell::load_pinned(app.handle());
    *app.state::<Mutex<GlassbarSettings>>()
        .lock()
        .expect("settings lock poisoned") = shell::load_settings(app.handle());
    // A "sleep" focus window armed before a shutdown must not leave
    // Do not disturb stuck on past 8:00 AM: reconcile it on boot.
    shell::dnd_boot_check(app.handle());
}

/// Configures the overlay window for the glass treatment: no decorations, a
/// transparent background, exclusion from capture, and layered composition.
/// Returns the overlay's HWND as `isize` for the shell APIs.
fn prepare_overlay_window(window: &WebviewWindow) -> Result<HWND, Box<dyn std::error::Error>> {
    window.set_decorations(false)?;
    window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)))?;
    let hwnd = HWND(window.hwnd()?.0 as *mut _);
    exclude_from_capture(hwnd)?;
    enable_layered_composition(hwnd);
    Ok(hwnd)
}

/// WDA_EXCLUDEFROMCAPTURE keeps the web UI out of the glass's shared WGC
/// feed: the capture must only ever contain the desktop, or the glass would
/// refract the pill's own DOM pixels. Display affinity is global per window,
/// so the pill is also invisible to other apps' recordings and screen shares.
fn exclude_from_capture(hwnd: HWND) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::WDA_EXCLUDEFROMCAPTURE,
        )?;
    }
    Ok(())
}

/// Layered is what lets WS_EX_TRANSPARENT actually pass mouse hits through
/// cross-thread; the hit-test loop toggles the transparent style so the
/// pill's buttons stay interactive while everything else (the oversized
/// overlay body) passes clicks to the windows behind.
fn enable_layered_composition(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
            WS_EX_LAYERED,
        };
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongW(hwnd, GWL_EXSTYLE, (style | WS_EX_LAYERED.0) as i32);
        let _ = SetLayeredWindowAttributes(hwnd, None, 255, LWA_ALPHA);
    }
}

/// Places the taskbar-sized glass pill: either claiming the bottom screen
/// edge through the AppBar API (when the saved config wants the reservation)
/// or floating over an untouched taskbar. Only claim the edge when the saved
/// config wants the reservation; previously this ran unconditionally, so a
/// disabled setting only took effect after the first toggle.
fn position_taskbar(app: &App, hwnd: isize) -> Result<(), Box<dyn std::error::Error>> {
    let reserve_space = app
        .state::<Mutex<GlassbarSettings>>()
        .lock()
        .expect("settings lock poisoned")
        .reserve_space;
    let width = app.state::<RendererState>().taskbar_content_width() as i32;

    if !reserve_space {
        eprintln!("[boot] reserve space disabled, placing floating pill");
        shell::reposition_unregistered(hwnd, width)
            .map_err(|error| format!("Failed to position Glassbar: {error}"))?;
        return Ok(());
    }

    shell::register_appbar(hwnd, width)
        .map_err(|error| format!("Failed to initialize AppBar: {error}"))?;
    eprintln!("[boot] appbar registered, subclassing");
    Ok(())
}

/// Attaches the GPU glass renderer to the overlay. A failure is logged and
/// swallowed: the web UI keeps working without the glass, degraded.
fn attach_renderer(app: &App, hwnd: isize) {
    if let Err(error) = app.state::<RendererState>().attach_to_overlay(HWND(hwnd as *mut _)) {
        eprintln!("Glassbar GPU renderer is unavailable: {error}");
    }
}

/// Spawns every long-lived background loop: the taskbar hider, the hit-test
/// supervision, the media session watcher, and the foreground tracker.
fn start_background_services(app_handle: &AppHandle, window: &WebviewWindow, hwnd: isize) {
    // Keeps the Windows taskbar hidden whenever Explorer re-shows it.
    shell::start_taskbar_monitor();
    app_handle.state::<RendererState>().set_emitter(app_handle.clone());
    eprintln!("[boot] starting hit test loop");
    spawn_hit_test_loop(window.clone(), app_handle.clone());
    shell::start_media_monitor(app_handle.clone());
    shell::start_foreground_tracker(hwnd);
    wheel_volume::spawn_wheel_volume(app_handle.clone(), hwnd);
}

/// Re-syncs the GPU surface bounds whenever the overlay window moves or
/// resizes, so the glass always sits behind the web UI.
fn track_overlay_bounds(window: &WebviewWindow, app_handle: AppHandle, hwnd: isize) {
    window.on_window_event(move |event| {
        if !matches!(
            event,
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
        ) {
            return;
        }
        if let Err(error) = app_handle
            .state::<RendererState>()
            .sync_overlay_bounds(HWND(hwnd as *mut _))
        {
            eprintln!("Could not synchronize the GPU surface bounds: {error}");
        }
    });
}

/// Tears everything down on exit: never leave the display warm, never leave
/// the user without their taskbar, and release the AppBar reservation.
fn handle_exit(app_handle: &AppHandle) {
    shell::reset_night_light();
    shell::restore_taskbar();
    app_handle.state::<RendererState>().destroy_surface();
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };
    if let Ok(hwnd) = window.hwnd() {
        shell::unregister_appbar(hwnd.0 as isize);
    }
}

/// The Tauri boot sequence: hydrate state, prepare the overlay window,
/// position the taskbar, attach the renderer, then start the services.
fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    apply_startup_state(app);
    let window = app
        .get_webview_window("main")
        .ok_or("Glassbar's main window was not created")?;
    eprintln!("[boot] window ready");

    let hwnd = prepare_overlay_window(&window)?;
    eprintln!("[boot] display affinity set (web UI excluded from capture)");

    position_taskbar(app, hwnd.0 as isize)?;
    shell::install_activation_subclass(hwnd.0 as isize)
        .map_err(|error| format!("Failed to subclass the overlay: {error}"))?;
    eprintln!("[boot] subclassed, attaching GPU renderer");

    attach_renderer(app, hwnd.0 as isize);
    eprintln!("[boot] renderer attached");
    apply_startup_settings(app, &app.state::<RendererState>());

    let app_handle = app.handle().clone();
    start_background_services(&app_handle, &window, hwnd.0 as isize);
    track_overlay_bounds(&window, app_handle, hwnd.0 as isize);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(RendererState::new())
        .manage(ShellState::default())
        .manage(Mutex::new(GlassbarSettings::default()))
        .plugin(tauri_plugin_opener::init())
        .on_page_load(|webview, payload| {
            eprintln!(
                "[page-load] label={} url={} event={:?}",
                webview.label(),
                payload.url(),
                payload.event()
            );
        })
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::shell_snapshot,
            commands::keyboard_layout,
            commands::set_ui_hit_rects,
            commands::activate_window,
            commands::close_app,
            commands::position_menu_glass,
            commands::sync_menu_glasses,
            commands::prewarm_menu_glass,
            commands::set_window_preview,
            commands::set_reserve_space,
            commands::set_taskbar_content_width,
            commands::get_settings,
            commands::set_settings,
            commands::reset_settings,
            commands::open_settings_window,
            commands::media_snapshot,
            commands::toggle_media_play,
            commands::media_next,
            commands::media_previous,
            commands::media_thumbnail,
            commands::media_timeline,
            commands::media_seek,
            commands::audio_snapshot,
            commands::set_audio_master,
            commands::set_audio_muted,
            commands::set_audio_app,
            commands::wifi_networks,
            commands::wifi_connect,
            commands::wifi_enabled,
            commands::set_wifi_enabled,
            commands::bluetooth_devices,
            commands::bluetooth_pair,
            commands::bluetooth_unpair,
            commands::bluetooth_enabled,
            commands::set_bluetooth_enabled,
            commands::wifi_disconnect,
            commands::set_airplane_mode,
            commands::set_night_light,
            commands::dnd_state,
            commands::set_dnd,
            commands::set_dnd_sleep,
            commands::launch_pinned,
            commands::pin_app,
            commands::unpin_app,
            commands::list_custom_gadgets,
            commands::read_custom_gadget,
            commands::open_gadgets_folder,
            commands::tray_icons,
            commands::tray_icon_activate,
            commands::tray_icon_context_menu,
            commands::hidden_windows,
            commands::reveal_hidden_window,
            commands::battery_snapshot,
            commands::exit_glassbar,
            commands::log_message
        ])
        .build(tauri::generate_context!())
        .expect("failed to start Glassbar");
    eprintln!("[boot] setup complete, entering event loop");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            handle_exit(app_handle);
        }
    });
}
