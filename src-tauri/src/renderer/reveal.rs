//! The auto-hidden taskbar's reveal rules, in one place.
//!
//! The reveal gesture is ONE trigger: the cursor entering an invisible strip
//! anchored to the screen's bottom edge (full monitor width, user-configured
//! height). The rules:
//!
//! 1. While the taskbar is parked (Desktop layer, or fully hidden), the
//!    strip is its REGULAR height — a fresh reveal always requires dipping
//!    into exactly that band, no matter how visible the parked pill is.
//! 2. While the taskbar FLOATS in the topmost band because of a reveal
//!    gesture (hovering, or a slide still in flight), the strip expands to
//!    its HOVERED height plus a small hide margin: the cursor can travel
//!    from the strip up into the pill (and back) without the bar dropping
//!    away mid-interaction. The band collapses as soon as the taskbar parks
//!    again, so the next reveal requires the regular height.
//!
//! "Floating" is fed back from the z-order system ([`super::layering`]):
//! the believed topmost band IS the floating state, so the strip's geometry
//! and the z-order rules can never disagree.

/// Sane floor for both strip heights (physical pixels).
pub const MIN_STRIP_PX: i32 = 8;
/// While floating, the cursor must climb this far above the expanded strip
/// before the taskbar may hide again — hysteresis, so hovering the strip's
/// top edge doesn't flicker.
pub const HIDE_MARGIN_PX: i32 = 4;

/// The two user-configured strip heights (physical pixels, measured up from
/// the screen's bottom edge).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RevealConfig {
    /// The regular strip height: the reveal gesture's trigger band.
    pub regular_px: i32,
    /// The expanded strip height while the taskbar floats.
    pub hover_px: i32,
}

impl RevealConfig {
    /// Clamps both heights to the sane floor and never lets the hovered
    /// height shrink below the regular one.
    pub fn new(hover_strip_height: f64, hover_strip_hover_height: f64) -> Self {
        let regular_px = hover_strip_height.max(MIN_STRIP_PX as f64) as i32;
        let hover_px = hover_strip_hover_height
            .max(MIN_STRIP_PX as f64)
            .max(hover_strip_height) as i32;
        Self {
            regular_px,
            hover_px,
        }
    }
}

/// Owns the believed strip state and answers the reveal question: "is the
/// cursor inside the reveal strip right now?"
#[derive(Default)]
pub struct Reveal {
    config: Option<RevealConfig>,
    /// Whether the expanded (hovered) band is active: true exactly while
    /// the taskbar floats in the topmost band.
    floating: bool,
}

impl Reveal {
    /// Feeds the current settings in; returns true when the config changed,
    /// so callers can log the change once instead of every tick.
    pub fn update(&mut self, config: RevealConfig) -> bool {
        let changed = self.config != Some(config);
        self.config = Some(config);
        changed
    }

    /// The believed z-order band feeds back into the strip geometry: the
    /// expanded (hovered) band is active exactly while the taskbar floats.
    pub fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
    }

    /// Whether the expanded (hovered) band is currently active.
    pub fn floating(&self) -> bool {
        self.floating
    }

    /// The strip's current height: the hovered height while floating, the
    /// regular height otherwise.
    pub fn band_px(&self) -> i32 {
        match self.config {
            Some(config) if self.floating => config.hover_px,
            Some(config) => config.regular_px,
            None => MIN_STRIP_PX,
        }
    }

    /// The strip's top edge (screen Y) for the given screen height: the
    /// band grows upward from the screen's bottom edge, plus the hide
    /// margin while floating.
    pub fn strip_top(&self, screen_height: i32) -> i32 {
        if self.floating {
            screen_height - self.band_px() - HIDE_MARGIN_PX
        } else {
            screen_height - self.band_px()
        }
    }

    /// Whether the cursor's screen Y is inside the strip (which spans the
    /// whole monitor width, so the taskbar reveals at the same height
    /// everywhere).
    pub fn contains(&self, cursor_y: i32, screen_height: i32) -> bool {
        cursor_y >= self.strip_top(screen_height)
    }
}
