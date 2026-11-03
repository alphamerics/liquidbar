//! The overlay supervision loop: click-through hit-testing, the auto-hide
//! slide, the reveal strip and the z-order band, ticking every 8 ms.
//!
//! The pieces it supervises live in their own modules: the click-through
//! hit-test ([`click_through`]), the slide animator (`renderer::Slide`),
//! the reveal strip rules (`renderer::reveal`) and the z-order band
//! (`renderer::layering`). What remains here is the per-tick orchestration:
//! sampling the cursor, foreground and settings state, feeding the rules,
//! and wiring their outputs together.

mod click_through;

use crate::renderer::{
    decide, slide_target, z_layer, Layer, LayerInputs, Layering, RendererState, Reveal,
    RevealConfig, Slide, SlideInputs,
};
use crate::settings::GlassbarSettings;
use crate::shell;
use tauri::Manager;
use windows::core::w;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, WaitForSingleObject,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS,
};

/// The supervision cadence while something needs fast response: the slide
/// animation is in flight, the cursor is on the pill/strip (reveal and
/// click-through latency), or a panel/menu is open.
const FAST_TICK: std::time::Duration = std::time::Duration::from_millis(8);
/// The idle cadence: with the cursor far from the bottom edge and nothing
/// animating, the loop's only duty is watching the reveal strip — a reveal
/// gesture is perceptible well above 100 ms, so a slower poll is invisible
/// while letting the CPU/core idle three out of four ticks. (A plain
/// `sleep(8)` on the default 15.6 ms timer granularity never actually woke
/// at 8 ms anyway; the waitable timer below makes the fast cadence real.)
const SLOW_TICK: std::time::Duration = std::time::Duration::from_millis(32);
/// Cursor distance from the screen's bottom edge inside which the loop runs
/// at the fast cadence (the reveal strip's approach window).
const STRIP_WATCH_PX: i32 = 240;

/// A precise one-shot sleep: a high-resolution waitable timer where
/// supported (no global `timeBeginPeriod` side effects), falling back to a
/// plain sleep elsewhere. The fast tick's 8 ms is otherwise impossible —
/// on the default 15.6 ms granularity a `sleep(8)` still waits a full slot.
struct Ticker {
    timer: Option<windows::Win32::Foundation::HANDLE>,
}

impl Ticker {
    fn new() -> Self {
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                w!("glassbar-supervision-tick"),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
        }
        .ok();
        Self { timer }
    }

    fn wait(&self, period: std::time::Duration) {
        match self.timer {
            Some(handle) => {
                // Negative due time = relative (100-ns ticks).
                let ticks = (period.as_nanos().min(i64::MAX as u128) / 100) as i64;
                let due = -ticks;
                unsafe {
                    SetWaitableTimer(handle, &due, 0, None, None, false).ok();
                    WaitForSingleObject(handle, u32::MAX);
                }
            }
            None => std::thread::sleep(period),
        }
    }
}

/// Lets mouse clicks fall through the tall overlay wherever the cursor is not
/// over the taskbar pill or an open menu. The overlay window stays one fixed
/// size, so menus never force a flicker-inducing resize.
pub fn spawn_hit_test_loop(window: tauri::WebviewWindow, renderer: tauri::AppHandle) {
    std::thread::spawn(move || {
        // The eased slide between the rest and hidden positions: the
        // animator owns the progress, timing and the choreography that
        // rides along (renderer::slide).
        let mut slide = Slide::new();
        // The believed z-order band owner: applies raise/lower on layer
        // changes and runs the 1 Hz stacking sanity check
        // (renderer::layering).
        let mut layering = Layering::new();
        // The resting layer decided each tick by the layering rules
        // (renderer::layering): drives both the slide target and the z-band.
        // The reveal-strip rule system (renderer::reveal): owns the band
        // heights, the floating feedback and the hysteresis margin, and
        // logs config changes (settings-window edits) once instead of
        // every tick.
        let mut reveal = Reveal::default();
        // Precise waits (see [`Ticker`]): the fast cadence needs real 8 ms
        // ticks, the idle cadence saves battery by sleeping longer.
        let ticker = Ticker::new();

        loop {
            // The next wait's cadence, decided by what this tick saw. Error
            // paths return the slow cadence so a failed query can never
            // spin the loop hot.
            let period = (|| {
                let Ok(webview_hwnd) = window.hwnd() else {
                    return SLOW_TICK;
                };
                let hwnd = windows::Win32::Foundation::HWND(webview_hwnd.0 as *mut _);
                unsafe {
                    use windows::Win32::{
                        Foundation::{POINT, RECT},
                        UI::WindowsAndMessaging::{
                            GetCursorPos, GetSystemMetrics, GetWindowRect, SM_CYSCREEN,
                        },
                    };
                    let mut cursor = POINT::default();
                    if GetCursorPos(&mut cursor).is_err() {
                        return SLOW_TICK;
                    }
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_err() {
                        return SLOW_TICK;
                    }
                    let renderer_state = renderer.state::<RendererState>();
                    let hits = renderer_state.menu_hit_rects();
                    // Drives the grouped glass reveal: held-back surfaces are
                    // released the moment every member of the group is warm.
                    renderer_state.check_pending_group();
                    let hover = click_through::CursorHover::measure(cursor, rect, &hits);
                    click_through::apply(hwnd, hover.interactive);
                    // The shared hover system's authoritative exit channel
                    // (shell::hover): the webview stops receiving input in
                    // exactly the tick its popups' hover beliefs can go
                    // stale, so the blur edge is derived here, where the
                    // real cursor and every hit rect are known.
                    crate::shell::hover::supervise(hover.interactive);
                    // Auto-hide: without a screen reservation the pill slides off the
                    // bottom of the screen when the cursor leaves it, and slides back
                    // when the cursor touches the screen's bottom edge. The renderer
                    // translates its surfaces by the same step so the glass slides
                    // with the pill (and its sampling origin tracks the window).
                    // The same slide machinery serves the reserved-space mode,
                    // where it only ever slides away for true fullscreen
                    // foreground apps (the layering rules decide when).
                    let autohide = renderer_state.autohide_enabled();
                    // ONE reveal trigger, uniform across the whole monitor
                    // width (both heights are user settings, physical pixels
                    // from the screen's bottom edge). The rules live in
                    // renderer::reveal: the reveal itself always happens at
                    // the regular strip height; only while the taskbar floats
                    // in the topmost band does the strip expand to its
                    // hovered height (so the cursor can travel from the strip
                    // into the pill without dropping the bar).
                    let settings_state = renderer.state::<std::sync::Mutex<GlassbarSettings>>();
                    let (strip_regular, strip_hover, hide_when_unhovered, reveal_in_fullscreen) = {
                        let guard = settings_state.lock().expect("settings lock poisoned");
                        (
                            guard.hover_strip_height,
                            guard.hover_strip_hover_height,
                            guard.hide_when_unhovered,
                            guard.reveal_in_fullscreen,
                        )
                    };
                    let strip_config = RevealConfig::new(strip_regular, strip_hover);
                    if reveal.update(strip_config) {
                        eprintln!(
                            "[reveal] strip config: height={}px hovered={}px",
                            strip_config.regular_px, strip_config.hover_px
                        );
                    }
                    let screen_height = GetSystemMetrics(SM_CYSCREEN);
                    let overlay_bottom = shell::overlay_bottom_y();
                    let rest_top = overlay_bottom - shell::overlay_height_px();
                    let hidden_top = screen_height;
                    // The expanded (hovered) band is active exactly while the
                    // taskbar floats — the z-order system's belief, one tick
                    // stale at worst (8 ms) — so a parked taskbar always
                    // reveals at exactly the regular strip height.
                    reveal.set_floating(layering.believed_topmost());
                    let strip_band_px = reveal.band_px();
                    // How much the foreground app covers: maximized windows
                    // are held off the strip by the AppBar reservation
                    // (auto-hide still slides away), while a true fullscreen
                    // app ignores reservations entirely and hides the
                    // taskbar in every mode.
                    let cover = shell::foreground_cover();
                    let fg_maximized = autohide && cover.maximized;
                    let fg_fullscreen = cover.fullscreen;
                    // The strip spans the whole monitor width, so the
                    // taskbar reveals at the same height everywhere. Under a
                    // true fullscreen app the strip only reveals while the
                    // mouse cursor is VISIBLE — games hide the cursor during
                    // gameplay, so camera-driven cursor drift into the strip
                    // is not a reveal gesture (moving the mouse while it is
                    // hidden can still land the cursor in the strip). With
                    // reveal-in-fullscreen off the strip is fully ignored
                    // under fullscreen apps.
                    let strip = reveal.contains(cursor.y, screen_height)
                        && (!cover.fullscreen || (reveal_in_fullscreen && shell::cursor_visible()));
                    // While the settings window is open the taskbar stays put
                    // no matter where the cursor goes — the window belongs to
                    // the pill and hiding one would hide the other.
                    let settings_open = renderer_state.settings_open();
                    // The reveal gesture is ONLY the strip: a parked taskbar
                    // (Desktop layer or hidden) reveals exclusively when the
                    // cursor dips into the regular strip height — hovering the
                    // pill's interactive elements keeps clicks working (the
                    // click-through toggle above) but never floats or reveals
                    // the bar. While floating, the expanded strip band — and
                    // the whole pill hover area beneath it — HOLD the bar up,
                    // so reaching buttons through the pill's empty areas (or
                    // hovering them above a small expanded band) doesn't drop
                    // the bar mid-interaction — but not while a hide slide is
                    // in flight, or a cursor parked on the pill would catch
                    // the descending bar and flap it back up forever. Open
                    // panels and previews hold the bar up on their own via
                    // menus_open.
                    let hiding = slide.hiding(screen_height);
                    let reach = strip
                        || ((hover.pill_hover || hover.interactive)
                            && layering.believed_topmost()
                            && !hiding);
                    // Open panels (the control center, context menus) float
                    // above the overlay: while any is open the taskbar must
                    // stay put and topmost, wherever the cursor goes.
                    let menus_open = renderer_state.has_open_menu_glass();
                    // The resting layer (the rules live in renderer::layering)
                    // drives both the slide target and, via z_layer, the
                    // z-order band below.
                    // The toggle hotkey (hotkey::taskbar_disabled) forces
                    // the Hidden layer in EVERY mode: hover, the reveal
                    // strip, open panels and the settings window are all
                    // ignored until the hotkey is pressed again.
                    let resting_layer = if crate::hotkey::taskbar_disabled() {
                        Layer::Hidden
                    } else {
                        decide(&LayerInputs {
                            autohide,
                            foreground_maximized: fg_maximized,
                            foreground_fullscreen: fg_fullscreen,
                            hovered: reach,
                            settings_open,
                            menus_open,
                            hide_when_unhovered,
                        })
                    };
                    let target_top = slide_target(resting_layer, rest_top, hidden_top);
                    slide.tick(
                        &renderer,
                        hwnd,
                        SlideInputs {
                            window_left: rect.left,
                            current_top: rect.top,
                            target_top,
                            screen_height,
                            cursor_above_bottom: screen_height - cursor.y,
                            strip_band_px,
                            floating: reveal.floating(),
                        },
                    );
                    // Z order (renderer::layering): while a slide toward the
                    // hidden position is in flight the taskbar stays in the
                    // topmost band (it slides away OVER the covering window);
                    // otherwise the resting layer decided above applies. In
                    // reserve mode the resting layer is Top except under a
                    // true fullscreen app, which fully hides the taskbar.
                    // Layering raises/lowers on change and runs the 1 Hz
                    // stacking sanity check itself.
                    let sliding_to_hidden = slide.hiding(screen_height);
                    layering.apply(
                        hwnd,
                        &renderer_state,
                        z_layer(resting_layer, sliding_to_hidden),
                    );
                    layering.enforce(hwnd, &renderer_state);
                    // The cadence: fast while the slide animates, the cursor
                    // is on the pill/strip (or approaching it), or a
                    // panel/menu needs the supervision; slow otherwise.
                    let animating = rect.top != target_top;
                    let near_strip = screen_height - cursor.y <= STRIP_WATCH_PX;
                    let busy = hover.interactive
                        || hover.pill_hover
                        || animating
                        || menus_open
                        || settings_open
                        || hiding;
                    if busy || near_strip {
                        FAST_TICK
                    } else {
                        SLOW_TICK
                    }
                }
            })();
            ticker.wait(period);
        }
    });
}
