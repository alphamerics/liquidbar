//! Renderer state is intentionally independent from Tauri commands and React.
//! The GPU pipeline lives in the reusable `liquid-glass` crate; the menu
//! glass machinery (per-key surfaces, recycle pool, group reveals, menu
//! springs, hit rects) lives in the shared `glass-ui` crate — this module
//! keeps only the taskbar-specific pieces: the pill surface, the hover
//! preview, the pill/preview springs and the z-order/sliding plumbing.

mod layering;
mod reveal;
mod slide;

pub(crate) use layering::{decide, slide_target, z_layer, LayerInputs, Layering};
pub(crate) use reveal::{Reveal, RevealConfig};
pub(crate) use slide::{Slide, SlideInputs};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, SetWindowPos,
        GWL_EXSTYLE, HWND_NOTOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOPMOST,
    },
};

use liquid_glass::{
    translate_compositor, translate_surface_window, GlassConfig, GlassSurface,
    SharedCompositor, SurfaceAnchor,
};
use tauri::Emitter;

use glass_ui::MenuGlassManager;
use glass_ui::MENU_REVEAL_INSTANT;

const TASKBAR_PILL_HEIGHT_PX: f32 = 68.0;
const TASKBAR_PILL_RADIUS_PX: f32 = 34.0;

/// The persistent hover-preview surface plus the state needed to skip
/// redundant re-anchoring (which would restack topmost windows and flicker).
struct PreviewGlass {
    surface: GlassSurface,
    rect: [f32; 4],
    radius: f32,
}

pub struct RendererState {
    /// The shared menu-glass machinery (per-key surfaces, pool, groups,
    /// menu springs, hit rects). Most glass commands delegate here.
    menus: MenuGlassManager,
    /// The hover preview's hit rect in screen coordinates, plus whether a
    /// preview is currently open. Kept glassbar-side: the preview is not a
    /// menu key, so the manager does not track it.
    preview_hit_rect: Mutex<Option<[i32; 4]>>,
    preview_open: Mutex<bool>,
    /// The persistent taskbar pill surface, shared with the resize-spring
    /// thread via Arc (the spring outlives any single method call).
    surface: std::sync::Arc<Mutex<Option<GlassSurface>>>,
    /// Whether the settings window is open: while it is, the taskbar must
    /// never auto-hide (the settings window belongs to the taskbar — hiding
    /// the pill would just hide the settings window out from under the user).
    settings_open: std::sync::atomic::AtomicBool,
    /// Whether the floating pill should slide off-screen when not hovered
    /// (true whenever the screen reservation is disabled).
    autohide: Mutex<bool>,
    /// True while the auto-hide slide animates: the overlay fires Moved
    /// events every step, and re-syncing all surfaces in response would fight
    /// the slide (and repaint) — translate_surfaces already keeps everything
    /// aligned during the slide.
    sliding: Mutex<bool>,
    /// One persistent, warm surface for the hover previews. It is created
    /// collapsed and never destroyed: opening re-anchors it and replays the
    /// reveal, so its capture session never needs to warm up again — a fresh
    /// WGC session's first frame takes hundreds of milliseconds to arrive.
    /// Shared via Arc so the resize spring can drive it from its own thread.
    preview_glass: std::sync::Arc<Mutex<Option<PreviewGlass>>>,
    /// The most recently applied shader settings. Every surface created
    /// afterwards (menus, previews) seeds from this, so glass that appears
    /// long after startup still matches the saved settings.
    shader_params: Mutex<liquid_glass::GlassShaderParams>,
    /// The z-order the taskbar should sit in while auto-hiding: true = in
    /// the topmost band (the user can reach it), false = parked beneath the
    /// foreground window (the taskbar is part of the desktop, behind any
    /// window that overlaps it). `sync_overlay_bounds` re-applies the
    /// lowered stacking whenever a Moved event re-raises everything.
    topmost: AtomicBool,
    /// The pill's desired width, reported by the webview from its measured
    /// content (physical pixels). The pill grows to fit the content and
    /// shrinks back when it needs less; the AppBar layer clamps the value
    /// to a sane floor and below the monitor width. Boot default matches
    /// the old fixed pill width so the first frame lands at a sane size.
    taskbar_content_width: Mutex<f64>,
    /// The pill's last-applied effective (clamped) width, physical pixels:
    /// resize springs are keyed on changes of THIS value, not the raw
    /// desired width (which can saturate at the monitor cap).
    taskbar_effective_width: Mutex<f64>,
    /// The width the overlay window actually landed on after the last
    /// reposition (the AppBar snap may adjust the requested rect). Drift
    /// healing compares against THIS value — comparing against the requested
    /// width would fight the snap in an endless reposition loop.
    taskbar_applied_width: Mutex<i32>,
    /// Live squash/stretch spring for the taskbar's resize animation, shared
    /// with its animation thread via Arc. `generation` supersedes stale
    /// threads when a new resize retriggers mid-flight.
    pill_spring: std::sync::Arc<Mutex<PillSpring>>,
    /// When the renderer attached to the overlay; width reports inside the
    /// boot-settle window don't spawn resize springs.
    attached_at: Mutex<Option<std::time::Instant>>,
    /// Cached resize-spring parameters (squash, bounce, duration_ms) applied
    /// from settings, so any code path (pill resize, preview re-anchor) can
    /// start a spring without a Tauri state handle.
    resize_anim: Mutex<(f64, f64, f64)>,
}

/// Which surface the resize spring animates. The menu arm is gone — menu
/// springs live in the shared `glass-ui` manager now.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SpringTarget {
    /// The taskbar pill (resize-to-fit spring).
    Pill,
    /// The hover-preview glass (spring when the preview re-anchors to an
    /// app button of a different width).
    Preview,
}

/// See [`RendererState::pill_spring`]: the live squash/stretch state of the
/// taskbar's resize spring. The scale is anchored to the pill's bottom-center
/// in the shader (x overshoots with the width change, y counter-squashes).
struct PillSpring {
    scale: (f32, f32),
    velocity: (f32, f32),
    active: bool,
    generation: u64,
    target: SpringTarget,
}

impl Default for PillSpring {
    fn default() -> Self {
        Self {
            scale: (1.0, 1.0),
            velocity: (0.0, 0.0),
            active: false,
            generation: 0,
            target: SpringTarget::Pill,
        }
    }
}

/// Spring integration constants shared by the native glass spring (this
/// file) and the DOM content spring (Taskbar.tsx) so both move as one:
/// ω = 2π/duration (duration reads as one full oscillation period), damping
/// ratio ζ = 1 − bounce·0.75 (bounce 1 = very bouncy, 0 = no overshoot).
fn pill_spring_constants(bounce: f64, duration_ms: f64) -> (f64, f64) {
    glass_ui::spring_constants(bounce, duration_ms)
}

/// Integrates one spring axis toward 1 with a hard ceiling at 1.0: the pill
/// may compress freely (values below 1) but never stretches beyond its
/// surface, so the animation can never clip at the window edges — reaching
/// 1 reflects the velocity with `restitution` instead, which reads as the
/// pill squashing into place and bouncing back.
fn pill_spring_step(
    value: f32,
    velocity: f32,
    omega: f64,
    zeta: f64,
    restitution: f32,
    dt: f32,
) -> (f32, f32) {
    glass_ui::spring_step(value, velocity, omega, zeta, restitution, dt)
}

impl RendererState {
    pub fn new() -> Self {
        Self {
            menus: MenuGlassManager::new(),
            preview_hit_rect: Mutex::new(None),
            preview_open: Mutex::new(false),
            surface: std::sync::Arc::new(Mutex::new(None)),
            settings_open: std::sync::atomic::AtomicBool::new(false),
            autohide: Mutex::new(false),
            sliding: Mutex::new(false),
            preview_glass: std::sync::Arc::new(Mutex::new(None)),
            shader_params: Mutex::new(liquid_glass::GlassShaderParams::default()),
            topmost: AtomicBool::new(true),
            taskbar_content_width: Mutex::new(520.0),
            taskbar_effective_width: Mutex::new(520.0),
            taskbar_applied_width: Mutex::new(0),
            pill_spring: std::sync::Arc::new(Mutex::new(PillSpring::default())),
            attached_at: Mutex::new(None),
            resize_anim: Mutex::new((0.4, 0.5, 320.0)),
        }
    }

    /// Wires the shared manager's group emitter to the web UI's
    /// `glass-group-ready` event (grouped panels fade their content in as
    /// one unit when their glass is warm).
    pub fn set_emitter(&self, app: tauri::AppHandle) {
        self.menus.set_group_emitter(move || {
            let _ = app.emit("glass-group-ready", ());
        });
    }

    /// Delegates to the shared manager (see `glass-ui::MenuGlassManager`).
    pub fn prewarm_glasses(&self, overlay: HWND, count: usize) -> Result<(), String> {
        self.menus.prewarm(overlay, count)
    }

    /// Renders (or removes) a native liquid glass surface behind one menu.
    /// The `preview*` key convention routes to the taskbar's persistent
    /// hover-preview surface instead of a menu instance.
    pub fn position_menu_glass(
        &self,
        overlay: HWND,
        key: &str,
        rect: Option<[f32; 4]>,
        radius: f32,
        instant: bool,
        glass_fade: bool,
        group: Option<&str>,
        spring: bool,
    ) -> Result<(), String> {
        if key.starts_with("preview") {
            return self.position_preview_glass(overlay, rect, radius);
        }
        self.menus.position_menu_glass(
            overlay,
            key,
            rect,
            radius,
            instant,
            glass_fade,
            group,
            spring,
        )
    }

    /// Delegates the webview's reconciliation sweep to the shared manager.
    pub fn sync_menu_glasses(&self, keys: &[String]) {
        self.menus.sync_glasses(keys);
    }

    /// Every hit rect the hit-test loop must honor: the open menus' (from
    /// the shared manager) plus the preview's when open.
    pub fn menu_hit_rects(&self) -> Vec<[i32; 4]> {
        let mut rects = self.menus.hit_rects();
        if let Some(rect) = *self.preview_hit_rect.lock().expect("preview rect lock poisoned") {
            rects.push(rect);
        }
        rects
    }

    /// The open panels' hit rects (open menus + the preview when shown),
    /// WITHOUT the web UI's interactive-element rects — the merged list is
    /// [`Self::menu_hit_rects`].
    pub fn open_panel_hit_rects(&self) -> Vec<[i32; 4]> {
        let mut rects = self.menus.open_hit_rects();
        if let Some(rect) = *self.preview_hit_rect.lock().expect("preview rect lock poisoned") {
            rects.push(rect);
        }
        rects
    }

    /// Delegates the pending group reveal check to the shared manager.
    pub fn check_pending_group(&self) {
        self.menus.check_pending_group();
    }

    /// Caches the resize-spring parameters from settings (applied by
    /// `apply_settings` on startup and on every settings change).
    pub fn set_resize_anim(&self, squash: f64, bounce: f64, duration_ms: f64) {
        *self.resize_anim.lock().expect("resize anim lock poisoned") =
            (squash, bounce, duration_ms);
        self.menus.set_spring_params(squash, bounce, duration_ms);
    }

    fn resize_anim_params(&self) -> (f64, f64, f64) {
        *self.resize_anim.lock().expect("resize anim lock poisoned")
    }

    /// The pill's desired (content-driven) width in physical pixels.
    pub fn taskbar_content_width(&self) -> f64 {
        *self
            .taskbar_content_width
            .lock()
            .expect("taskbar content width lock poisoned")
    }

    /// Updates the desired content width, returning the previous value so
    /// callers can skip work when nothing changed.
    pub fn set_taskbar_content_width(&self, width: f64) -> f64 {
        let mut guard = self
            .taskbar_content_width
            .lock()
            .expect("taskbar content width lock poisoned");
        let previous = *guard;
        *guard = width;
        previous
    }

    /// Tracks the pill's effective (clamped) width, returning the previous
    /// value so callers can detect a real on-screen resize.
    pub fn set_effective_taskbar_width(&self, width: f64) -> f64 {
        let mut guard = self
            .taskbar_effective_width
            .lock()
            .expect("taskbar effective width lock poisoned");
        let previous = *guard;
        *guard = width;
        previous
    }

    /// Records the width the overlay window actually landed on, and reports
    /// the previously recorded one (0 before the first apply).
    pub fn set_applied_taskbar_width(&self, width: i32) -> i32 {
        let mut guard = self
            .taskbar_applied_width
            .lock()
            .expect("taskbar applied width lock poisoned");
        let previous = *guard;
        *guard = width;
        previous
    }

    /// The last recorded on-screen width (0 before the first apply).
    pub fn applied_taskbar_width(&self) -> i32 {
        *self
            .taskbar_applied_width
            .lock()
            .expect("taskbar applied width lock poisoned")
    }

    /// Starts the pill's squash-and-stretch spring for a resize from
    /// `from_w` to `to_w` physical pixels. Returns `Some(ratio)` — the raw
    /// from/to width ratio — for the caller to hand to the DOM spring via
    /// the `pill-resize-anim` event (which derives the same initial
    /// conditions), or `None` when the spring was suppressed (boot settle:
    /// the webview is still measuring its content, so width reports churn
    /// and must not spawn animation after animation).
    ///
    /// Initial conditions never exceed 1.0 on either axis: growing starts
    /// compressed (x = from/to) with a velocity kick proportional to the
    /// size change, so bigger resizes rebound harder off the ceiling; a
    /// shrink starts flush with a proportional inward kick. The ceiling
    /// clamp in [`pill_spring_step`] keeps every value ≤ 1.
    pub fn trigger_pill_resize_spring(
        &self,
        target: SpringTarget,
        from_w: f64,
        to_w: f64,
        squash: f64,
        bounce: f64,
        duration_ms: f64,
    ) -> Option<f64> {
        if self.boot_settling() {
            return None;
        }
        let to = to_w.max(1.0);
        let ratio = (from_w / to).clamp(0.5, 2.0);
        // Velocity-proportional overshoot: the spring starts with a velocity
        // kick proportional to the size change, so a bigger resize rebounds
        // harder off the ceiling clamp (the "squash into place" bounce) or
        // dips proportionally deeper on a shrink.
        let (omega, zeta) = pill_spring_constants(bounce, duration_ms);
        let delta = (ratio - 1.0).abs();
        let (x_initial, vx_initial) = if ratio <= 1.0 {
            (ratio as f32, (delta * omega) as f32)
        } else {
            (1.0_f32, -((delta * omega) as f32))
        };
        // Compressed start on y (never above 1 — above 1 would clip): the
        // vertical squash mirrors the width displacement, scaled by the
        // user's squash amount.
        let amplitude = (ratio - 1.0).abs().clamp(0.0, 0.35);
        let y_initial = (1.0 - amplitude * (0.5 + 0.5 * squash)) as f32;
        let restitution = (bounce.clamp(0.0, 1.0) * 0.6) as f32;
        let (spring, surface, preview_glass) = (
            std::sync::Arc::clone(&self.pill_spring),
            std::sync::Arc::clone(&self.surface),
            std::sync::Arc::clone(&self.preview_glass),
        );
        let generation = {
            let mut state = spring.lock().expect("pill spring lock poisoned");
            state.generation += 1;
            state.scale = (x_initial, y_initial);
            state.velocity = (vx_initial, 0.0);
            state.active = true;
            state.target = target;
            state.generation
        };
        std::thread::spawn(move || {
            // Hard cap so a pathologically underdamped spring can't tick
            // forever: four full oscillation periods.
            let max_ticks = ((duration_ms.max(60.0) / 8.0) * 4.0) as u32;
            for _ in 0..max_ticks {
                // Integrate with the REAL inter-tick interval: sleep(8ms)
                // actually sleeps ~15.6ms on Windows (timer granularity), and
                // feeding the claimed 8ms into the integrator while twice the
                // real time passes desyncs the glass from the DOM spring —
                // visible as jitter.
                let tick_start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(8));
                let dt = tick_start.elapsed().as_secs_f32().min(0.05);
                let (scale, settled) = {
                    let mut state = spring.lock().expect("pill spring lock poisoned");
                    if state.generation != generation || !state.active {
                        return;
                    }
                    let (x, vx) = pill_spring_step(
                        state.scale.0,
                        state.velocity.0,
                        omega,
                        zeta,
                        restitution,
                        dt,
                    );
                    let (y, vy) = pill_spring_step(
                        state.scale.1,
                        state.velocity.1,
                        omega,
                        zeta,
                        restitution,
                        dt,
                    );
                    let settled = (x - 1.0).abs() < 0.002
                        && (y - 1.0).abs() < 0.002
                        && vx.abs() < 0.01
                        && vy.abs() < 0.01;
                    if settled {
                        state.scale = (1.0, 1.0);
                        state.velocity = (0.0, 0.0);
                        state.active = false;
                        ((1.0, 1.0), true)
                    } else {
                        state.scale = (x, y);
                        state.velocity = (vx, vy);
                        ((x, y), false)
                    }
                };
                match target {
                    SpringTarget::Pill => {
                        let mut surface = surface.lock().expect("renderer surface lock poisoned");
                        if let Some(glass) = surface.as_mut() {
                            glass.set_pill_scale_xy(scale.0, scale.1);
                        }
                    }
                    SpringTarget::Preview => {
                        let mut preview =
                            preview_glass.lock().expect("preview glass lock poisoned");
                        if let Some(glass) = preview.as_mut() {
                            glass.surface.set_pill_scale_xy(scale.0, scale.1);
                        }
                    }
                }
                if settled {
                    return;
                }
            }
            // Cap reached: snap to rest and stop.
            let mut state = spring.lock().expect("pill spring lock poisoned");
            if state.generation == generation {
                state.scale = (1.0, 1.0);
                state.velocity = (0.0, 0.0);
                state.active = false;
            }
            match target {
                SpringTarget::Pill => {
                    let mut surface = surface.lock().expect("renderer surface lock poisoned");
                    if let Some(glass) = surface.as_mut() {
                        glass.set_pill_scale_xy(1.0, 1.0);
                    }
                }
                SpringTarget::Preview => {
                    let mut preview = preview_glass.lock().expect("preview glass lock poisoned");
                    if let Some(glass) = preview.as_mut() {
                        glass.surface.set_pill_scale_xy(1.0, 1.0);
                    }
                }
            }
        });
        Some(ratio)
    }

    /// Whether the renderer recently attached and the webview is still
    /// measuring its content: width reports churn during this window and
    /// must not trigger resize springs.
    fn boot_settling(&self) -> bool {
        const BOOT_SETTLE: std::time::Duration = std::time::Duration::from_secs(2);
        self.attached_at
            .lock()
            .expect("attached-at lock poisoned")
            .map(|attached| attached.elapsed() < BOOT_SETTLE)
            .unwrap_or(true)
    }

    /// Replaces the webview-pushed interactive-element rects (overlay-relative
    /// physical pixels). Stored in the menu manager so one hit-rects fetch
    /// serves the hit-test loop.
    pub fn set_ui_hit_rects(&self, rects: Vec<[i32; 4]>) {
        self.menus.set_ui_hit_rects(rects);
    }

    /// Auto-hide (slide off-screen when not hovered) is enabled whenever the
    /// screen reservation is disabled, i.e. the pill floats.
    pub fn set_autohide(&self, enabled: bool) {
        *self.autohide.lock().expect("autohide lock poisoned") = enabled;
    }

    pub fn autohide_enabled(&self) -> bool {
        *self.autohide.lock().expect("autohide lock poisoned")
    }

    /// Tracks whether the settings window is open; the hit-test loop keeps
    /// the taskbar visible while it is.
    pub fn set_settings_open(&self, open: bool) {
        self.settings_open
            .store(open, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn settings_open(&self) -> bool {
        self.settings_open
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The hit-test loop marks the slide's span so overlay Moved events don't
    /// trigger full surface re-syncs mid-animation.
    pub fn set_sliding(&self, sliding: bool) {
        *self.sliding.lock().expect("sliding lock poisoned") = sliding;
    }

    /// Scales the pill's glass shape during the auto-hide reveal (1 = full).
    pub fn set_pill_reveal_scale(&self, scale: f32) {
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_mut()
        {
            surface.set_pill_scale(scale);
        }
    }

    /// Parks (or wakes) the taskbar pill's glass. While the auto-hidden pill
    /// is fully off-screen its glass is invisible, so it must not keep
    /// rendering content-driven frames (and while it is parked, the shared
    /// capture skips its full-screen copy too). The next reveal wakes it.
    pub fn set_pill_hidden(&self, hidden: bool) {
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_mut()
        {
            surface.set_paused(hidden);
        }
    }

    /// Every glass surface's window handle (pill, menus, recycled pool,
    /// preview), for z-order sanity checks.
    pub fn surface_hwnd_set(&self) -> Vec<isize> {
        self.collect_surface_targets()
            .into_iter()
            .map(|(hwnd, _)| hwnd)
            .collect()
    }

    /// Collects every glass surface's window handle and compositor. The
    /// caller must release all renderer locks before using the handles (see
    /// the deadlock notes on [`Self::translate_surfaces`]).
    fn collect_surface_targets(&self) -> Vec<(isize, SharedCompositor)> {
        let mut targets: Vec<(isize, SharedCompositor)> = Vec::new();
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_ref()
        {
            targets.push((surface.hwnd_addr(), surface.compositor()));
        }
        targets.extend(self.menus.surface_targets());
        if let Some(preview) = self
            .preview_glass
            .lock()
            .expect("preview glass lock poisoned")
            .as_ref()
        {
            targets.push((preview.surface.hwnd_addr(), preview.surface.compositor()));
        }
        targets
    }

    /// Shifts every surface (and thus the glass visuals) vertically during the
    /// auto-hide slide. All anchors are overlay-relative, so translating the
    /// overlay and every surface by the same amount keeps the geometry intact.
    pub fn translate_surfaces(&self, dy: i32) {
        if dy == 0 {
            return;
        }
        let targets = self.collect_surface_targets();
        for (hwnd, compositor) in targets {
            let _ = translate_surface_window(hwnd, dy);
            translate_compositor(&compositor, dy);
        }
    }

    /// The z-order the taskbar should sit in while auto-hiding.
    pub fn set_desired_topmost(&self, topmost: bool) {
        self.topmost.store(topmost, Ordering::Relaxed);
    }

    pub fn desired_topmost(&self) -> bool {
        self.topmost.load(Ordering::Relaxed)
    }

    /// Whether any floating panel is currently shown above the overlay (the
    /// control center's cards, context menus, or a hover preview). While
    /// one is open the taskbar must stay put and topmost — the panel would
    /// otherwise sink beneath the foreground window with the pill.
    pub fn has_open_menu_glass(&self) -> bool {
        self.menus.has_open_glass()
            || *self.preview_open.lock().expect("preview open lock poisoned")
    }

    /// Stacks every glass surface directly beneath `insert_after`. When
    /// raising, the overlay must ALREADY be topmost, so the surfaces join
    /// the topmost band directly below it; when lowering, `insert_after` is
    /// the foreground window and the overlay is expected to join them
    /// afterwards (see [`Self::lower_z_order`]).
    ///
    /// All locks are released before any window operation — `SetWindowPos`
    /// blocks until the surface window's owning thread pumps messages, and
    /// that thread may be waiting on a renderer lock (deadlock).
    pub fn set_surfaces_z_order(&self, insert_after_hwnd: isize) {
        for (hwnd_addr, _) in self.collect_surface_targets() {
            unsafe {
                let _ = SetWindowPos(
                    HWND(hwnd_addr as *mut _),
                    HWND(insert_after_hwnd as *mut _),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// Parks the overlay and every glass surface just beneath the foreground
    /// window (or at the top of the normal band when nothing is foreground,
    /// the foreground is topmost-band, or the foreground is the desktop).
    ///
    /// The surfaces sink first — the overlay is still topmost above them, so
    /// the DOM never sits under its own glass — and the overlay joins them
    /// directly below the foreground window last.
    pub fn lower_z_order(&self, overlay: HWND) {
        unsafe {
            let fg = GetForegroundWindow();
            let fg_in_topmost_band =
                (GetWindowLongW(fg, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
            // Never park beneath the desktop itself: Progman/WorkerW paint
            // the wallpaper, and a sibling inserted below them vanishes
            // behind it. The desktop is not a window that overlaps the
            // taskbar anyway.
            let mut fg_class = [0u16; 16];
            let class: String = if fg.0.is_null() {
                String::new()
            } else {
                let len = GetClassNameW(fg, &mut fg_class);
                String::from_utf16_lossy(&fg_class[..len.max(0) as usize])
            };
            let fg_is_desktop = class == "Progman" || class == "WorkerW";
            let insert_after =
                if fg.0.is_null() || fg.0 == overlay.0 || fg_in_topmost_band || fg_is_desktop {
                    HWND_NOTOPMOST
                } else {
                    fg
                };
            for (hwnd_addr, _) in self.collect_surface_targets() {
                let _ = SetWindowPos(
                    HWND(hwnd_addr as *mut _),
                    insert_after,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            let _ = SetWindowPos(
                overlay,
                insert_after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub fn attach_to_overlay(&self, overlay: HWND) -> Result<(), String> {
        *self.attached_at.lock().expect("attached-at lock poisoned") =
            Some(std::time::Instant::now());
        // The overlay can grow upward for menus; the glass always covers only
        // the bottom taskbar pill strip with capsule-rounded corners.
        let config = GlassConfig {
            corner_radius_px: TASKBAR_PILL_RADIUS_PX,
            surface_anchor: SurfaceAnchor::BottomStrip {
                height_px: TASKBAR_PILL_HEIGHT_PX,
            },
            ..Default::default()
        };
        // Cap the pill's content-driven frame rate at 60fps. Reveal
        // and slide animations still render at the tick rate (the throttle
        // only applies to content-driven frames).
        let mut surface = GlassSurface::create(overlay.0 as isize, config)?;
        surface.set_frame_cap(Some(60.0));
        // The pill is the one always-visible surface, so its pacing is
        // adaptive: content-driven frames render at the shared loop's full
        // rate while the captured desktop keeps changing (moving content
        // under the glass stays smooth), then settle back to the 60fps cap
        // once the content goes quiet.
        surface.set_adaptive_rate(true);
        *self.surface.lock().expect("renderer surface lock poisoned") = Some(surface);
        // Pre-warm the hover-preview surface, collapsed and parked at a 1x1
        // rect, so previews appear instantly the first time they are used.
        let config = GlassConfig {
            corner_radius_px: 20.0,
            surface_anchor: SurfaceAnchor::Rect {
                offset_x: 0.0,
                offset_y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            // The pill's own surface already styled the host overlay.
            style_host_window: false,
            ..Default::default()
        };
        match GlassSurface::create(overlay.0 as isize, config) {
            Ok(mut preview) => {
                preview.set_reveal(false, MENU_REVEAL_INSTANT);
                preview.set_frame_cap(Some(60.0));
                // Warm the capture session with one frame, then stop until
                // the first preview actually shows.
                preview.pause_after_next_present();
                *self
                    .preview_glass
                    .lock()
                    .expect("preview glass lock poisoned") = Some(PreviewGlass {
                    surface: preview,
                    rect: [0.0, 0.0, 1.0, 1.0],
                    radius: 20.0,
                });
            }
            Err(error) => eprintln!("Glassbar preview glass pre-warm failed: {error}"),
        }
        Ok(())
    }

    /// Shows (or collapses) the persistent hover-preview surface. Unlike menu
    /// glass, the preview's surface is never destroyed: `None` collapses it
    /// with the reversed reveal animation, keeping its capture session warm so
    /// the next preview appears instantly. Pushes with an unchanged rect are
    /// no-ops — re-anchoring would restack the topmost windows (surface, then
    /// overlay) and make the surface jump above the thumbnail popup, which
    /// reads as flicker.
    fn position_preview_glass(
        &self,
        overlay: HWND,
        rect: Option<[f32; 4]>,
        radius: f32,
    ) -> Result<(), String> {
        let mut preview = self
            .preview_glass
            .lock()
            .expect("preview glass lock poisoned");
        let Some(rect) = rect else {
            if let Some(preview) = preview.as_mut() {
                // No reveal theatrics: the preview glass just disappears
                // instantly (its DOM is already gone), then stops rendering
                // once the instant collapse lands.
                preview.surface.set_reveal(false, MENU_REVEAL_INSTANT);
                preview.surface.collapse_then_pause();
            }
            *self.preview_hit_rect.lock().expect("preview rect lock poisoned") = None;
            *self.preview_open.lock().expect("preview open lock poisoned") = false;
            return Ok(());
        };
        let preview = match preview.as_mut() {
            Some(preview) => preview,
            None => {
                let config = GlassConfig {
                    corner_radius_px: radius,
                    surface_anchor: SurfaceAnchor::Rect {
                        offset_x: rect[0],
                        offset_y: rect[1],
                        width: rect[2],
                        height: rect[3],
                    },
                    // The pill's own surface already styled the host
                    // overlay; preview spawns must not re-style it.
                    style_host_window: false,
                    ..Default::default()
                };
                let mut surface = match GlassSurface::create(overlay.0 as isize, config) {
                    Ok(surface) => surface,
                    Err(error) => {
                        eprintln!("Glassbar preview glass failed: {error}");
                        return Err(error);
                    }
                };
                // Seed the fresh surface with the saved glass settings so it
                // matches the rest of the composition.
                let _ = surface.set_shader_params_keeping_corner(
                    *self
                        .shader_params
                        .lock()
                        .expect("shader params lock poisoned"),
                );
                surface.set_frame_cap(Some(60.0));
                surface.set_reveal(false, MENU_REVEAL_INSTANT);
                // Warm the capture session with a single frame, then stop.
                surface.pause_after_next_present();
                preview.insert(PreviewGlass {
                    surface,
                    rect,
                    radius,
                })
            }
        };
        // A visible preview renders (throttled); a parked one does not.
        preview.surface.set_paused(false);
        let moved = !preview
            .rect
            .iter()
            .zip(rect.iter())
            .all(|(a, b)| (a - b).abs() < 2.0);
        if moved {
            preview
                .surface
                .set_anchor(SurfaceAnchor::Rect {
                    offset_x: rect[0],
                    offset_y: rect[1],
                    width: rect[2],
                    height: rect[3],
                })
                .map_err(|error| format!("Could not move the preview glass: {error}"))?;
            preview.rect = rect;
            // The SDF's corner radius is normalized to the surface height,
            // which just changed (the surface is pre-created at 1x1 and
            // clamped to a full capsule) — re-apply it explicitly.
            preview
                .surface
                .set_corner_radius(radius)
                .map_err(|error| format!("Could not round the preview glass: {error}"))?;
            preview.radius = radius;
            // The re-anchor restacked the glass surface above the thumbnail
            // popup; put the popup back between the glass and the overlay.
            if let Err(error) = crate::shell::preview::restack(overlay.0 as isize) {
                eprintln!("Glassbar preview restack failed: {error}");
            }
        }
        if (preview.radius - radius).abs() >= 0.5 {
            preview
                .surface
                .set_corner_radius(radius)
                .map_err(|error| format!("Could not round the preview glass: {error}"))?;
            preview.radius = radius;
        }
        // No reveal animation: the preview glass appears fully open and the
        // resize spring provides the scale-up bounce, exactly like the
        // taskbar pill's resize animation.
        preview.surface.set_reveal(true, MENU_REVEAL_INSTANT);
        if moved {
            // Every appearance or re-anchor plays the scale-up bounce. A
            // width-ratio spring would be a no-op here (re-anchors keep the
            // preview's own width), so the spring always starts compressed
            // at 0.9 and bounces into the full size.
            let (squash, bounce, duration_ms) = self.resize_anim_params();
            self.trigger_pill_resize_spring(
                SpringTarget::Preview,
                rect[2] as f64 * 0.9,
                rect[2] as f64,
                squash,
                bounce,
                duration_ms,
            );
        }
        // Record the preview's hit rect in screen coordinates so the hit-test
        // loop catches clicks on it while it is open.
        let mut host = RECT::default();
        unsafe {
            let _ = GetWindowRect(overlay, &mut host);
        }
        let hit_rect = [
            host.left + rect[0] as i32,
            host.top + rect[1] as i32,
            host.left + (rect[0] + rect[2]) as i32,
            host.top + (rect[1] + rect[3]) as i32,
        ];
        *self
            .preview_hit_rect
            .lock()
            .expect("preview rect lock poisoned") = Some(hit_rect);
        *self
            .preview_open
            .lock()
            .expect("preview open lock poisoned") = true;
        Ok(())
    }

    /// Updates the taskbar pill's corner radius live.
    pub fn set_taskbar_corner_radius(&self, radius_px: f32) -> Result<(), String> {
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_mut()
        {
            surface.set_corner_radius(radius_px)?;
        }
        Ok(())
    }

    /// Applies shared shader settings to every surface; each surface keeps
    /// its own corner radius. The params are also stored, so every surface
    /// created later (menus, previews) inherits them — glass that exists only
    /// after startup stays consistent with the applied settings.
    pub fn apply_glass_settings(
        &self,
        params: liquid_glass::GlassShaderParams,
    ) -> Result<(), String> {
        *self
            .shader_params
            .lock()
            .expect("shader params lock poisoned") = params;
        let mut errors = Vec::new();
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_mut()
        {
            if let Err(error) = surface.set_shader_params_keeping_corner(params) {
                errors.push(error);
            }
        }
        self.menus.set_params(params);
        if let Some(preview) = self
            .preview_glass
            .lock()
            .expect("preview glass lock poisoned")
            .as_mut()
        {
            if let Err(error) = preview.surface.set_shader_params_keeping_corner(params) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn destroy_surface(&self) {
        self.menus.destroy();
        self.preview_glass
            .lock()
            .expect("preview glass lock poisoned")
            .take();
        self.surface
            .lock()
            .expect("renderer surface lock poisoned")
            .take();
    }

    pub fn sync_overlay_bounds(&self, overlay: HWND) -> Result<(), String> {
        // During the auto-hide slide the overlay fires Moved events every
        // step; translate_surfaces already keeps everything aligned, and a
        // full re-sync here would repaint-storm the animation.
        if *self.sliding.lock().expect("sliding lock poisoned") {
            return Ok(());
        }
        if let Some(surface) = self
            .surface
            .lock()
            .expect("renderer surface lock poisoned")
            .as_mut()
        {
            surface.sync_bounds(overlay.0 as isize)?;
        }
        self.menus.sync_bounds(overlay);
        if let Some(preview) = self
            .preview_glass
            .lock()
            .expect("preview glass lock poisoned")
            .as_mut()
        {
            preview.surface.sync_bounds(overlay.0 as isize)?;
        }
        // A Moved event can land right after the taskbar was parked beneath
        // the foreground window; re-apply the lowered stacking so the glass
        // never pops back above the windows. (When topmost is desired, the
        // stack_beneath_host calls above already raised everything.)
        if !self.desired_topmost() {
            self.lower_z_order(overlay);
        }
        Ok(())
    }
}
