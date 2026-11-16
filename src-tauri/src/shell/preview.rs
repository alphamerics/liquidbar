//! Live taskbar hover previews, rendered by our own GPU pipeline.
//!
//! A small top-level popup window is placed over the webview's preview image
//! area and shows a live capture of the target app window (the GPU-rendered
//! rounded thumbnail surface from `liquid_glass`). The popup is borderless,
//! never activates, and stays out of alt-tab. The popup reports its own mouse
//! hover back to the webview (the image area sits above it, so the webview
//! cannot see the cursor there) and forwards clicks as window activation.
//!
//! Threading: the main thread owns the popup and does only window operations
//! (placement, show/hide, the hover report). EVERY thumbnail surface
//! operation — creating the D3D/DComposition pipeline, retargeting the
//! capture, resizing the swap chain, and the teardown — runs on a dedicated
//! worker: those calls synchronize with the Windows capture pipeline and
//! DWM, and a wedged pipeline must stall only the worker, never the
//! taskbar's main thread (the whole app once froze exactly at the hide).

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc;
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

/// The single queued request, tagged with the generation it was written
/// under. Writes always replace the slot; every write schedules one
/// main-thread closure.
static PREVIEW_REQUEST: Mutex<Option<(u64, Option<PreviewRequest>)>> = Mutex::new(None);
/// Bumped per write; each write schedules exactly one closure under that
/// generation.
static PREVIEW_GENERATION: AtomicU64 = AtomicU64::new(0);
/// The newest generation whose request actually ran. A closure that finds
/// the slot empty only hides when its own generation never ran — that keeps
/// fast hover (open A, open B before A's closure runs) from turning into
/// show-then-hide flip-flops.
static PREVIEW_EXECUTED: AtomicU64 = AtomicU64::new(0);
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
/// Schedules one main-thread closure (which owns both the overlay and the
/// popup and pumps their messages); the closure does the window operations
/// and hands the thumbnail-surface work to the surface worker.
///
/// The request slot is a single mailbox, but generations make a queued
/// update always carry the caller's CURRENT intent: a write from hover B
/// replaces hover A's pending request; A's scheduled closure takes the slot
/// and runs B's request (executed generation moves to the newest write).
/// Closures that find the slot empty do nothing when their generation
/// already ran, so rapid hovering can never order a run as show-then-hide.
pub fn set_preview(
    window: &tauri::WebviewWindow,
    overlay_hwnd: isize,
    target: Option<isize>,
    rect: Option<[i32; 4]>,
    corner_radius: i32,
) -> Result<(), String> {
    let _ = APP_HANDLE.get_or_init(|| window.app_handle().clone());
    let generation = {
        let mut slot = PREVIEW_REQUEST
            .lock()
            .map_err(|_| "preview request lock poisoned")?;
        let generation = PREVIEW_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        *slot = Some((
            generation,
            match (target, rect) {
                (Some(target), Some(rect)) => Some(PreviewRequest {
                    target,
                    rect,
                    corner_radius,
                }),
                _ => None,
            },
        ));
        generation
    };
    window
        .run_on_main_thread(move || {
            let taken = PREVIEW_REQUEST.lock().ok().and_then(|mut slot| slot.take());
            match taken {
                Some((generation, request)) => {
                    PREVIEW_EXECUTED.fetch_max(generation, Ordering::Relaxed);
                    match request {
                        Some(request) => unsafe { place(overlay_hwnd, request) },
                        None => hide(),
                    }
                }
                // Empty slot: a concurrent closure already processed the
                // newest request. Only hide if this closure's own generation
                // never ran (our request was consumed without executing) —
                // otherwise our scheduled hide would cancel a newer show.
                None => {
                    if PREVIEW_EXECUTED.load(Ordering::Relaxed) < generation {
                        hide();
                    }
                }
            }
        })
        .map_err(|error| format!("Could not schedule the preview update: {error}"))
}

/// One queued thumbnail-surface operation. The single worker runs them in
/// arrival order, which preserves the main thread's intent order (the
/// generation mailbox guarantees exactly one closure — hence one job — per
/// write, and the event loop runs closures in write order).
enum SurfaceJob {
    Show {
        popup: isize,
        request: PreviewRequest,
    },
    Hide,
}

static SURFACE_JOBS: OnceLock<Mutex<mpsc::Sender<SurfaceJob>>> = OnceLock::new();

/// Hands `job` to the surface worker, spawning it on first use. The send
/// cannot meaningfully fail: a dead worker (a panic already doomed the
/// process under the release profile) only drops the job.
fn enqueue_surface(job: SurfaceJob) {
    let sender = SURFACE_JOBS.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<SurfaceJob>();
        std::thread::Builder::new()
            .name("glassbar-preview-surface".to_owned())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    match job {
                        SurfaceJob::Show { popup, request } => apply_show(popup, request),
                        SurfaceJob::Hide => apply_hide(),
                    }
                }
            })
            .expect("could not spawn the preview surface worker");
        Mutex::new(sender)
    });
    if let Ok(sender) = sender.lock() {
        let _ = sender.send(job);
    }
}

/// The MAIN-THREAD half of showing a preview: window operations only
/// (create/reposition the popup). Everything that touches the thumbnail's
/// GPU/capture state is handed to the surface worker (`SurfaceJob`) —
/// WGC object creation, `ResizeBuffers` (which waits on the DWM) and the
/// capture pipeline's own synchronization must never run where a stall
/// freezes the whole taskbar: the worker stalls alone, and the next hover
/// retries. This is the same hazard class the reaper and the detached
/// capture starts already fence off; the show/hide surface ops were the
/// last main-thread participants left.
unsafe fn place(overlay: isize, request: PreviewRequest) {
    let preview = match ensure_window() {
        Ok(preview) => preview,
        Err(error) => {
            eprintln!("Glassbar window preview failed: {error}");
            return;
        }
    };
    let mut host = RECT::default();
    let _ = GetWindowRect(HWND(overlay as *mut _), &mut host);
    if let Err(error) = SetWindowPos(
        preview,
        HWND_TOPMOST,
        host.left + request.rect[0],
        host.top + request.rect[1],
        request.rect[2],
        request.rect[3],
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    ) {
        eprintln!("Glassbar window preview failed: could not place the preview window: {error}");
        return;
    }
    CURRENT_TARGET.store(request.target, Ordering::Relaxed);
    enqueue_surface(SurfaceJob::Show {
        popup: preview.0 as isize,
        request,
    });
}

/// The worker half of showing: all thumbnail surface state (create,
/// retarget, restyle, resize, show). The GPU thumbnail surface is
/// persistent; retargeting restarts the capture when the source changed.
/// Its shader owns the corner radius, so any value works. Every failure is
/// tolerated (retry on the next hover) rather than panicked on, and none
/// escalates: a failed preview leaves an empty popup at worst.
fn apply_show(popup: isize, request: PreviewRequest) {
    let Ok(mut slot) = THUMB.lock() else {
        // Poisoned: skip this preview (the next hover opens a fresh one)
        // instead of panicking.
        eprintln!("Glassbar preview thumbnail lock poisoned");
        return;
    };
    let surface = if let Some(surface) = slot.as_ref() {
        surface
    } else {
        match ThumbnailSurface::create(popup) {
            Ok(surface) => slot.get_or_insert(surface),
            Err(error) => {
                eprintln!("Glassbar preview thumbnail failed to create: {error}");
                return;
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
}

/// The MAIN-THREAD half of hiding: the popup's visibility and the hover
/// report only. The capture teardown (which synchronizes with the capture
/// pipeline — a wedged pipeline must never be waited on here) runs on the
/// surface worker, after this job in the queue.
fn hide() {
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
    enqueue_surface(SurfaceJob::Hide);
}

/// The worker half of hiding: stops the capture session and detaches the
/// source, so the capture pipeline idles with the hidden preview.
fn apply_hide() {
    if let Ok(slot) = THUMB.lock() {
        if let Some(surface) = slot.as_ref() {
            surface.set_visible(false);
            if let Err(error) = surface.set_source(0) {
                eprintln!("Glassbar preview thumbnail release failed: {error}");
            }
        }
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
