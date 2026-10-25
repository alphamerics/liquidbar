use std::ffi::c_void;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    },
    UI::{
        Shell::{
            DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM, ABM_NEW, ABM_REMOVE,
            ABM_SETPOS, APPBARDATA, SUBCLASSPROC,
        },
        WindowsAndMessaging::{
            GetSystemMetrics, GetWindowLongW, GetWindowRect, SetWindowLongW, SetWindowPos,
            GWL_EXSTYLE, HTTRANSPARENT, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WM_NCACTIVATE,
            WM_NCHITTEST, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE,
            WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE,
        },
    },
};

pub const TASKBAR_HEIGHT_PX: i32 = 68;
pub const TASKBAR_FLOAT_MARGIN_PX: i32 = 8;
pub const MENU_SURFACE_HEIGHT_PX: i32 = 480;
/// The pill grows to fit its content, but must never reach the full monitor:
/// this margin stays free on each side so it can never overflow the screen.
pub const TASKBAR_EDGE_MARGIN_PX: i32 = 48;
/// Sane floor for the pill's width when the reported content is tiny.
pub const TASKBAR_MIN_WIDTH_PX: i32 = 220;
/// The overlay window is permanently tall so menus can render above the
/// pill without resizing (which would flicker); the glass only covers the
/// bottom pill strip.
pub const OVERLAY_HEIGHT_PX: i32 = TASKBAR_HEIGHT_PX + MENU_SURFACE_HEIGHT_PX;
const OVERLAY_SUBCLASS_ID: usize = 1;

/// tao keeps WS_CAPTION in the overlay's style even though its WM_NCCALCSIZE
/// handler removes the frame. DefWindowProc still repaints the classic frame
/// whenever the window is deactivated, so intercept WM_NCACTIVATE and forward
/// it with lParam(-1), which suppresses the non-client repaint while tao's own
/// procedure (invoked through DefSubclassProc) keeps updating focus state.
unsafe extern "system" fn overlay_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if message == WM_NCACTIVATE {
        return DefSubclassProc(hwnd, WM_NCACTIVATE, wparam, LPARAM(-1));
    }
    // While the hit-test loop marks the overlay click-through
    // (WS_EX_TRANSPARENT), answer HTTRANSPARENT: cross-thread hit-testing does
    // not honor the style alone (DefWindowProc would return HTCLIENT and the
    // oversized overlay would swallow clicks on everything behind it).
    if message == WM_NCHITTEST
        && (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0) != 0
    {
        return LRESULT(HTTRANSPARENT as isize);
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub fn install_activation_subclass(hwnd: isize) -> Result<(), String> {
    let subclass: SUBCLASSPROC = Some(overlay_subclass_proc);
    unsafe {
        SetWindowSubclass(HWND(hwnd as *mut _), subclass, OVERLAY_SUBCLASS_ID, 0)
            .ok()
            .map_err(|error| format!("Could not subclass the overlay window: {error}"))
    }
}

/// The pill's on-screen width: the content-driven desired width, clamped to
/// a sane floor and capped so it can never reach the full monitor width.
pub fn taskbar_width_px(desired_width_px: i32) -> i32 {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let max_width = (screen_width - 2 * TASKBAR_EDGE_MARGIN_PX).max(TASKBAR_MIN_WIDTH_PX);
    desired_width_px.clamp(TASKBAR_MIN_WIDTH_PX, max_width)
}

/// The overlay's last-settled bottom edge (screen coordinates). The AppBar
/// snap result tracks the real taskbar's visibility, so re-deriving the
/// bottom on every registration let the bar teleport up and down; anchoring
/// to the first settled value keeps it put.
static OVERLAY_BOTTOM: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn register(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: HWND(hwnd as *mut _),
        ..Default::default()
    };

    unsafe {
        configure_borderless_host(data.hWnd)?;
        if SHAppBarMessage(ABM_NEW, &mut data) == 0 {
            return Err("Windows could not register Glassbar as an AppBar".to_string());
        }

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        // The content-driven desired width, clamped to sane bounds and never
        // wider than the screen minus its side margins.
        let taskbar_width = taskbar_width_px(desired_width_px);
        let taskbar_left = (screen_width - taskbar_width) / 2;
        // Reserve a strip tall enough for the floating bar plus the gap below
        // it, so maximized windows still avoid the whole area.
        data.uEdge = ABE_BOTTOM;
        data.rc = RECT {
            left: taskbar_left,
            top: screen_height - TASKBAR_HEIGHT_PX - TASKBAR_FLOAT_MARGIN_PX,
            right: taskbar_left + taskbar_width,
            bottom: screen_height,
        };
        let _ = SHAppBarMessage(ABM_SETPOS, &mut data);
        // Reuse the bottom edge the bar already settled at (if any) instead
        // of re-deriving it from the snap, which follows the real taskbar's
        // visibility (auto-hide popups included) and teleported the bar.
        let mut overlay_bottom = OVERLAY_BOTTOM.load(std::sync::atomic::Ordering::Relaxed);
        if overlay_bottom == 0 {
            overlay_bottom = screen_height - TASKBAR_FLOAT_MARGIN_PX;
            OVERLAY_BOTTOM.store(overlay_bottom, std::sync::atomic::Ordering::Relaxed);
        }
        SetWindowPos(
            data.hWnd,
            HWND_TOPMOST,
            data.rc.left,
            overlay_bottom - OVERLAY_HEIGHT_PX,
            data.rc.right - data.rc.left,
            OVERLAY_HEIGHT_PX,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|error| format!("Could not position Glassbar: {error}"))?;
        eprintln!(
            "[appbar] registered: snapped=({},{})..({},{}), placed left={} bottom={overlay_bottom} width={}",
            data.rc.left,
            data.rc.top,
            data.rc.right,
            data.rc.bottom,
            data.rc.left,
            taskbar_width
        );
    }

    Ok(())
}

/// The overlay's settled bottom edge (screen Y) — the auto-hide slide's rest
/// position derives from it.
pub fn overlay_bottom_y() -> i32 {
    let bottom = OVERLAY_BOTTOM.load(std::sync::atomic::Ordering::Relaxed);
    if bottom == 0 {
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        screen_height - TASKBAR_FLOAT_MARGIN_PX
    } else {
        bottom
    }
}

/// The overlay top a width change should keep: the rest position when the
/// window is at rest, or the CURRENT slide position when the taskbar is
/// hidden or mid-slide (auto-hide). Re-anchoring to the rest position on a
/// width change would yank a hidden taskbar fully on-screen for a
/// split-second reveal every time content changed.
unsafe fn width_change_top(hwnd: HWND, rest_top: i32) -> i32 {
    let mut current = RECT::default();
    let _ = GetWindowRect(hwnd, &mut current);
    if (current.top - rest_top).abs() <= 1 {
        rest_top
    } else {
        current.top
    }
}

/// Places the overlay bottom-centered WITHOUT an AppBar reservation: the
/// pill simply floats above other windows. Used at boot when the reservation
/// is disabled, so the window still lands in the right spot with the right
/// size instead of the tauri.conf.json defaults.
pub fn reposition_unregistered(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    unsafe {
        configure_borderless_host(HWND(hwnd as *mut _))?;
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let width = taskbar_width_px(desired_width_px);
        let left = (screen_width - width) / 2;
        let mut overlay_bottom = OVERLAY_BOTTOM.load(std::sync::atomic::Ordering::Relaxed);
        if overlay_bottom == 0 {
            overlay_bottom = screen_height - TASKBAR_FLOAT_MARGIN_PX;
            OVERLAY_BOTTOM.store(overlay_bottom, std::sync::atomic::Ordering::Relaxed);
        }
        let top = width_change_top(HWND(hwnd as *mut _), overlay_bottom - OVERLAY_HEIGHT_PX);
        SetWindowPos(
            HWND(hwnd as *mut _),
            HWND_TOPMOST,
            left,
            top,
            width,
            OVERLAY_HEIGHT_PX,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|error| format!("Could not position Glassbar: {error}"))
    }
}

unsafe fn configure_borderless_host(hwnd: HWND) -> Result<(), String> {
    // tao already strips the frame via its WM_NCCALCSIZE handler for
    // undecorated windows and always keeps WS_CAPTION in its style bits, so
    // never rewrite GWL_STYLE here (that fights tao and hides the window).
    // Also avoid DwmEnableBlurBehindWindow: on Win10/11 it re-enables DWM
    // non-client rendering and resurrects a title bar on borderless windows.
    let current_ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let frame_ex_style =
        WS_EX_DLGMODALFRAME.0 | WS_EX_WINDOWEDGE.0 | WS_EX_CLIENTEDGE.0 | WS_EX_STATICEDGE.0;
    let _ = SetWindowLongW(
        hwnd,
        GWL_EXSTYLE,
        (current_ex_style & !frame_ex_style) as i32,
    );

    // Keep DWM's non-client rendering enabled: tao's WM_NCCALCSIZE handler
    // already removes the frame, and disabling DWM NC rendering (NCRP_DISABLED)
    // makes DefWindowProc fall back to the classic GDI frame on NC repaints.
    let corner_preference = DWMWCP_DONOTROUND;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        (&corner_preference as *const DWM_WINDOW_CORNER_PREFERENCE).cast::<c_void>(),
        size_of_val(&corner_preference) as u32,
    )
    .map_err(|error| format!("Could not disable Windows corner rendering: {error}"))?;
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
    )
    .map_err(|error| format!("Could not remove Glassbar's native caption: {error}"))
}

/// Re-positions an already-registered appbar to a new width WITHOUT
/// unregistering: ABM_REMOVE lets the real taskbar reclaim the bottom edge
/// and the shell then shoves our window above it, permanently lifting the
/// bar. Width changes only need ABM_SETPOS plus our own SetWindowPos.
pub fn reposition(hwnd: isize, desired_width_px: i32) -> Result<(), String> {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: HWND(hwnd as *mut _),
        uEdge: ABE_BOTTOM,
        ..Default::default()
    };

    unsafe {
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let taskbar_width = taskbar_width_px(desired_width_px);
        let taskbar_left = (screen_width - taskbar_width) / 2;
        data.rc = RECT {
            left: taskbar_left,
            top: screen_height - TASKBAR_HEIGHT_PX - TASKBAR_FLOAT_MARGIN_PX,
            right: taskbar_left + taskbar_width,
            bottom: screen_height,
        };
        let _ = SHAppBarMessage(ABM_SETPOS, &mut data);
        let mut overlay_bottom = OVERLAY_BOTTOM.load(std::sync::atomic::Ordering::Relaxed);
        if overlay_bottom == 0 {
            overlay_bottom = screen_height - TASKBAR_FLOAT_MARGIN_PX;
            OVERLAY_BOTTOM.store(overlay_bottom, std::sync::atomic::Ordering::Relaxed);
        }
        let top = width_change_top(data.hWnd, overlay_bottom - OVERLAY_HEIGHT_PX);
        SetWindowPos(
            data.hWnd,
            HWND_TOPMOST,
            data.rc.left,
            top,
            data.rc.right - data.rc.left,
            OVERLAY_HEIGHT_PX,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|error| format!("Could not reposition Glassbar: {error}"))?;
    }

    Ok(())
}

pub fn unregister(hwnd: isize) {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: HWND(hwnd as *mut _),
        ..Default::default()
    };
    unsafe {
        let _ = SHAppBarMessage(ABM_REMOVE, &mut data);
    }
}
