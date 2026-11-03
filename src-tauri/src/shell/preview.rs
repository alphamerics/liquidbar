//! Live taskbar hover previews, rendered by our own GPU pipeline.
//!
//! A small top-level popup window is placed over the webview's preview image
//! area and shows a live capture of the target app window (the GPU-rendered
//! rounded thumbnail surface from `liquid_glass`). The popup is borderless,
//! never activates, and stays out of alt-tab. The popup reports its own mouse
//! hover back to the webview (the image area sits above it, so the webview
//! cannot see the cursor there) and forwards clicks as window activation.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use liquid_glass::ThumbnailSurface;

use windows::core::{w, PCWSTR};
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DwmSetWindowAttribute, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED,
            DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
            DWM_WINDOW_CORNER_PREFERENCE,
        },
        Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, GetWindowLongW, GetWindowRect, RegisterClassW,
            SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongW, SetWindowPos,
            ShowWindow, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE, HMENU, HTTRANSPARENT, HWND_TOPMOST,
            LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE,
            WDA_EXCLUDEFROMCAPTURE, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WNDCLASSW,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
};

/// Re-asserts the preview popup's position in the topmost band: directly
/// below the overlay (and above the glass surface beneath the overlay). When
/// the glass surface re-anchors it restacks itself above the popup, which
/// hides the thumbnail behind the opaque glass interior until this fixes it.
pub fn restack(overlay: isize) -> Result<(), String> {
    let popup = PREVIEW_HWND.load(Ordering::Relaxed);
    if popup == 0 {
        return Ok(());
    }
    unsafe {
        SetWindowPos(
            HWND(popup as *mut _),
            HWND(overlay as *mut _),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
        .map_err(|error| format!("Could not restack the preview popup: {error}"))
    }
}

/// One queued preview update: which window to show, and where (the overlay-
/// relative image-area rect in physical pixels), plus the thumbnail's corner
/// radius (physical px), drawn by the SDF mask in the thumbnail shader.
#[derive(Clone, Copy)]
struct PreviewRequest {
    target: isize,
    rect: [i32; 4],
    corner_radius: i32,
}

static PREVIEW_REQUEST: Mutex<Option<PreviewRequest>> = Mutex::new(None);
static PREVIEW_HWND: AtomicIsize = AtomicIsize::new(0);
/// The persistent GPU thumbnail surface bound to the popup; created lazily
/// with the popup and reused across previews (the swap chain resizes with
/// the popup, the capture retargets with the hovered app).
static THUMB: Mutex<Option<ThumbnailSurface>> = Mutex::new(None);
static CURRENT_TARGET: AtomicIsize = AtomicIsize::new(0);
static MOUSE_INSIDE: AtomicBool = AtomicBool::new(false);
static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

use tauri::Manager;

/// Shows (or hides) the live preview. `target` is the app window's HWND and
/// `rect` the preview image area, both in the overlay's own coordinates.
/// Runs on the main thread, which owns both the overlay and the popup and
/// pumps their messages.
pub fn set_preview(
    window: &tauri::WebviewWindow,
    overlay_hwnd: isize,
    target: Option<isize>,
    rect: Option<[i32; 4]>,
    corner_radius: i32,
) -> Result<(), String> {
    let _ = APP_HANDLE.get_or_init(|| window.app_handle().clone());
    *PREVIEW_REQUEST
        .lock()
        .expect("preview request lock poisoned") = match (target, rect) {
        (Some(target), Some(rect)) => Some(PreviewRequest {
            target,
            rect,
            corner_radius,
        }),
        _ => None,
    };
    window
        .run_on_main_thread(move || {
            let request = PREVIEW_REQUEST
                .lock()
                .expect("preview request lock poisoned")
                .take();
            match request {
                Some(request) => {
                    if let Err(error) = unsafe { show(overlay_hwnd, request) } {
                        eprintln!("Glassbar window preview failed: {error}");
                    }
                }
                None => hide(),
            }
        })
        .map_err(|error| format!("Could not schedule the preview update: {error}"))
}

unsafe fn show(overlay: isize, request: PreviewRequest) -> Result<(), String> {
    let preview = ensure_window()?;
    let mut host = RECT::default();
    let _ = GetWindowRect(HWND(overlay as *mut _), &mut host);
    SetWindowPos(
        preview,
        HWND_TOPMOST,
        host.left + request.rect[0],
        host.top + request.rect[1],
        request.rect[2],
        request.rect[3],
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    )
    .map_err(|error| format!("Could not place the preview window: {error}"))?;

    CURRENT_TARGET.store(request.target, Ordering::Relaxed);

    // The GPU thumbnail surface is persistent; retarget it (which restarts
    // the capture when the source changed), restyle it and size it to the
    // fresh rect. Its shader owns the corner radius, so any value works.
    let mut slot = THUMB.lock().expect("preview thumbnail lock poisoned");
    let surface = if let Some(surface) = slot.as_ref() {
        surface
    } else {
        match ThumbnailSurface::create(preview.0 as isize) {
            Ok(surface) => slot.get_or_insert(surface),
            Err(error) => {
                eprintln!("Glassbar preview thumbnail failed to create: {error}");
                return Ok(());
            }
        }
    };
    if let Err(error) = surface.set_source(request.target) {
        eprintln!("Glassbar preview thumbnail retarget failed: {error}");
    }
    surface.set_radius(request.corner_radius.max(0) as f32);
    if let Err(error) = surface.sync_window_size() {
        eprintln!("Glassbar preview thumbnail resize failed: {error}");
    }
    surface.set_visible(true);
    Ok(())
}

fn hide() {
    if let Ok(slot) = THUMB.lock() {
        if let Some(surface) = slot.as_ref() {
            surface.set_visible(false);
            if let Err(error) = surface.set_source(0) {
                eprintln!("Glassbar preview thumbnail release failed: {error}");
            }
        }
    }
    CURRENT_TARGET.store(0, Ordering::Relaxed);
    let hwnd = PREVIEW_HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
        }
    }
    if MOUSE_INSIDE.swap(false, Ordering::Relaxed) {
        emit_hover(false);
    }
}

unsafe fn ensure_window() -> Result<HWND, String> {
    let existing = PREVIEW_HWND.load(Ordering::Relaxed);
    if existing != 0 {
        return Ok(HWND(existing as *mut _));
    }
    register_class();
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        w!("GlassbarAppPreview"),
        w!(""),
        WS_POPUP,
        0,
        0,
        10,
        10,
        HWND::default(),
        HMENU::default(),
        HINSTANCE(
            GetModuleHandleW(PCWSTR::null())
                .map_err(|error| format!("Could not get the module handle: {error}"))?
                .0,
        ),
        None,
    )
    .map_err(|error| format!("Could not create the preview window: {error}"))?;
    PREVIEW_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    // Layered + transparent: clicks pass straight through the popup to the
    // overlay above it (which handles the preview box's interaction).
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    SetWindowLongW(
        hwnd,
        GWL_EXSTYLE,
        (style | WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as i32,
    );
    let _ = SetLayeredWindowAttributes(hwnd, None, 255, LWA_ALPHA);
    // The thumbnail surface draws the corners itself (SDF mask at the exact
    // requested radius): the Windows 11 system corner rounding must NOT also
    // round this popup, or the composition would double-clip the content.
    let preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        (&preference as *const DWM_WINDOW_CORNER_PREFERENCE).cast::<std::ffi::c_void>(),
        std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
    );
    // Drop the DWM frame and shadow: the glass around the popup samples the
    // screen, and a shadow band would bleed a dark halo into it.
    let policy: DWMNCRENDERINGPOLICY = DWMNCRP_DISABLED;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_NCRENDERING_POLICY,
        (&policy as *const DWMNCRENDERINGPOLICY).cast::<std::ffi::c_void>(),
        std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
    );
    // Display affinity removes the popup AND its DWM thumbnail from monitor
    // captures and shows the desktop behind, so the glass around the preview
    // can never sample the preview's own pixels.
    if let Err(error) = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) {
        eprintln!("Glassbar preview exclusion failed: {error}");
    }
    Ok(hwnd)
}

fn register_class() {
    CLASS_REGISTERED.get_or_init(|| unsafe {
        let instance = GetModuleHandleW(PCWSTR::null())
            .map(|module| HINSTANCE(module.0))
            .unwrap_or_default();
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(preview_wndproc),
            hInstance: instance,
            // No background brush: the popup itself must never paint, so no
            // flat fill can appear around the DWM thumbnail (which also keeps
            // the glass from sampling any black the popup could produce).
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
            lpszClassName: w!("GlassbarAppPreview"),
            ..Default::default()
        };
        RegisterClassW(&class);
    });
}

/// The webview cannot see the cursor while it is over this popup, so the
/// popup reports its own hover state; the webview uses it to keep the preview
/// open while the user looks at (or reaches) the thumbnail.
fn emit_hover(hovering: bool) {
    if let Some(app) = APP_HANDLE.get() {
        use tauri::Emitter;
        let _ = app.emit("preview-hover", hovering);
    }
}

/// Clears a stale popup hover latch without an event. Windows fires
/// `WM_MOUSELEAVE` only from mouse input, never from a window moving out
/// from under a parked cursor, so a repositioned popup can leave the latch
/// stuck true. The shared `hover-blur` edge (`shell::hover::supervise`) is
/// the webview-facing signal for that same crossing — it routes into the
/// same webview hover system — so this reset deliberately never emits a
/// second, duplicate leave.
pub fn reset_hover_latch() {
    MOUSE_INSIDE.store(false, Ordering::Relaxed);
}

unsafe extern "system" fn preview_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Clicks descend from the overlay (which sits above the popup and handles
    // the preview box's interaction); the popup itself must never swallow any.
    if message == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    match message {
        WM_MOUSEMOVE => {
            if !MOUSE_INSIDE.swap(true, Ordering::Relaxed) {
                emit_hover(true);
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            MOUSE_INSIDE.store(false, Ordering::Relaxed);
            emit_hover(false);
            LRESULT(0)
        }
        // Clicking the thumbnail activates the app being previewed.
        WM_LBUTTONUP => {
            let target = CURRENT_TARGET.load(Ordering::Relaxed);
            if target != 0 {
                let _ = crate::shell::activate(target, false);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut paint);
            let _ = EndPaint(hwnd, &paint);
            return LRESULT(0);
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
