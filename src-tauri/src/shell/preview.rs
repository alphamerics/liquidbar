//! Live taskbar hover previews, backed by DWM thumbnails.
//!
//! A small top-level popup window is placed over the webview's preview image
//! area and hosts a DWM thumbnail of the target app window. The popup is
//! borderless, never activates, and stays out of alt-tab; DWM composites the
//! thumbnail live with zero per-frame work. The popup reports its own mouse
//! hover back to the webview (the image area sits above it, so the webview
//! cannot see the cursor there) and forwards clicks as window activation.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail,
            DwmUpdateThumbnailProperties, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED,
            DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE,
            DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DWM_WINDOW_CORNER_PREFERENCE,
        },
        Gdi::{BeginPaint, CreateRoundRectRgn, EndPaint, SetWindowRgn, PAINTSTRUCT},
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongW, GetWindowRect,
            RegisterClassW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongW,
            SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE, HMENU, HTTRANSPARENT,
            HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
            SW_HIDE, WDA_EXCLUDEFROMCAPTURE, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT,
            WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_EX_TRANSPARENT, WS_POPUP,
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
/// radius (physical px) applied via the window region — layered windows clip
/// DWM thumbnails to their region, so the preview's corners can match the
/// glass box exactly.
#[derive(Clone, Copy)]
struct PreviewRequest {
    target: isize,
    rect: [i32; 4],
    corner_radius: i32,
}

static PREVIEW_REQUEST: Mutex<Option<PreviewRequest>> = Mutex::new(None);
static PREVIEW_HWND: AtomicIsize = AtomicIsize::new(0);
static THUMBNAIL: AtomicUsize = AtomicUsize::new(0);
static REGISTERED_TARGET: AtomicIsize = AtomicIsize::new(0);
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

    let target = HWND(request.target as *mut _);
    if REGISTERED_TARGET.load(Ordering::Relaxed) != request.target {
        let previous = THUMBNAIL.swap(0, Ordering::Relaxed);
        if previous != 0 {
            let _ = DwmUnregisterThumbnail(previous as isize);
        }
        let handle = DwmRegisterThumbnail(preview, target)
            .map_err(|error| format!("Could not register the preview thumbnail: {error}"))?;
        THUMBNAIL.store(handle as usize, Ordering::Relaxed);
        REGISTERED_TARGET.store(request.target, Ordering::Relaxed);
    }

    let handle = THUMBNAIL.load(Ordering::Relaxed);
    if handle == 0 {
        return Ok(());
    }
    let mut destination = RECT::default();
    GetClientRect(preview, &mut destination)
        .map_err(|error| format!("Could not measure the preview window: {error}"))?;
    let destination_width = (destination.right - destination.left).max(1);
    let destination_height = (destination.bottom - destination.top).max(1);
    let mut source = RECT::default();
    let _ = GetClientRect(target, &mut source);
    let properties = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION
            | DWM_TNP_RECTSOURCE
            | DWM_TNP_VISIBLE
            | DWM_TNP_OPACITY
            | DWM_TNP_SOURCECLIENTAREAONLY,
        fVisible: true.into(),
        fSourceClientAreaOnly: true.into(),
        opacity: 255,
        rcDestination: destination,
        rcSource: crop_to_aspect(source, destination_width as f32 / destination_height as f32),
        ..Default::default()
    };
    DwmUpdateThumbnailProperties(handle as isize, &properties)
        .map_err(|error| format!("Could not update the preview thumbnail: {error}"))?;

    // Layered windows clip their DWM thumbnails to the window region, so the
    // rounded region gives the thumbnail the exact corner radius requested
    // (glass corner radius + the popup's padding). The region is applied
    // after the thumbnail properties so the recomposite cannot race past it,
    // and with a redraw so DWM re-evaluates the shape immediately.
    let width = request.rect[2].max(1);
    let height = request.rect[3].max(1);
    let radius = request.corner_radius.clamp(0, width / 2).max(1);
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
    if SetWindowRgn(preview, region, true) == 0 {
        eprintln!("Glassbar preview region failed (falling back to DWM rounding)");
    }
    Ok(())
}

/// Crops the source rect around its center so the thumbnail fills the
/// destination box without stretching.
fn crop_to_aspect(source: RECT, aspect: f32) -> RECT {
    let width = (source.right - source.left).max(1);
    let height = (source.bottom - source.top).max(1);
    let (crop_width, crop_height) = if width as f32 / height as f32 > aspect {
        ((height as f32 * aspect).round() as i32, height)
    } else {
        (width, (width as f32 / aspect).round() as i32)
    };
    let left = source.left + (width - crop_width) / 2;
    let top = source.top + (height - crop_height) / 2;
    RECT {
        left,
        top,
        right: left + crop_width,
        bottom: top + crop_height,
    }
}

fn hide() {
    let handle = THUMBNAIL.swap(0, Ordering::Relaxed);
    if handle != 0 {
        unsafe {
            let _ = DwmUnregisterThumbnail(handle as isize);
        }
    }
    REGISTERED_TARGET.store(0, Ordering::Relaxed);
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
    // Base rounding comes from the window region (set per show) for the
    // exact glass-matching radius; on systems where a DWM thumbnail ignores
    // the region, the Windows 11 corner preference still rounds the whole
    // window composition (at the system radius) so the preview is never
    // left with square corners.
    let preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
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
            let target = REGISTERED_TARGET.load(Ordering::Relaxed);
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
