//! The taskbar's z-layering and reveal rules, in one place.
//!
//! The taskbar occupies one of three layers:
//!
//! - [`Layer::Desktop`] — visible at its rest position but parked just
//!   beneath the foreground window: part of the desktop, covered by
//!   whatever overlaps it.
//! - [`Layer::Top`] — above every window. This is the hovering state (and
//!   the state while panels are open or a slide is in flight).
//! - [`Layer::Hidden`] — slid off-screen entirely.
//!
//! The rules that map the desktop state onto a layer:
//!
//! 1. With a screen reservation (AppBar), the taskbar always sits in the
//!    topmost band, like a real taskbar.
//! 2. With **no maximized or fullscreen application** in the foreground,
//!    the taskbar is visible on the Desktop layer; hovering lifts it to
//!    Top, and when hovering stops it goes back to Desktop.
//! 3. With a **maximized application** in the foreground (auto-hide mode),
//!    the taskbar is actually Hidden; hovering brings it back up to Top.
//! 4. With a **true fullscreen application** in the foreground the taskbar
//!    fully hides in EVERY mode — a fullscreen surface covers the whole
//!    monitor and ignores AppBar reservations, so even the reserved-space
//!    taskbar slides away (hovering brings it back).
//! 5. While the **settings window is open** the taskbar stays put (it must
//!    not slide away — the window belongs to the pill), but it only claims
//!    the topmost band when a covering window would otherwise hide it.
//!    Normal foreground windows simply overlap it, as they would a parked
//!    pill — editing settings must not pin the bar above every window.
//! 6. With the optional **hide-when-unhovered** setting the taskbar fully
//!    hides whenever it is not hovered (and no menu or the settings window
//!    keeps it up), in every mode — the stricter sibling of rule 3 that
//!    ignores whether a maximized window actually covers the strip. A
//!    reveal gesture brings it back.
//!
//! Enforcement lives here too: a 1 Hz sanity check verifies the overlay's
//! actual topmost style and the glass surfaces' stacking chain against the
//! believed layer, repairing drift caused by interleaved window operations.
//! The reveal strip's geometry consumes the believed band (see
//! [`super::reveal`]).

use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetWindow, GetWindowLongW, SetWindowPos, GWL_EXSTYLE, GW_HWNDNEXT, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOPMOST,
    },
};

use super::RendererState;

/// The three layers the taskbar occupies (see the module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// Visible at the rest position, parked beneath app windows.
    Desktop,
    /// Above every window: hovered, panels open, or mid-slide.
    Top,
    /// Slid off-screen (maximized or fullscreen foreground, not hovered).
    Hidden,
}

/// The desktop state the layer decision consumes.
pub struct LayerInputs {
    /// The screen reservation mode: the taskbar behaves like a real
    /// taskbar and always claims the topmost band.
    pub autohide: bool,
    /// The foreground window is maximized: it covers the work area above
    /// the taskbar strip, which the AppBar reservation protects.
    pub foreground_maximized: bool,
    /// The foreground window is true fullscreen: it covers the whole
    /// monitor and ignores AppBar reservations, so the taskbar fully hides
    /// in every mode (a reveal gesture still brings it back).
    pub foreground_fullscreen: bool,
    /// Hover is active: the bottom strip always triggers a fresh reveal
    /// (at the regular strip height); once floating, the expanded strip
    /// band and hovering the pill's interactive elements hold the bar up
    /// (a hide slide in flight suppresses the interactive hold).
    pub hovered: bool,
    pub settings_open: bool,
    pub menus_open: bool,
    /// The optional hide-when-unhovered setting: the taskbar fully hides
    /// whenever it is not hovered (see rule 6 in the module docs).
    pub hide_when_unhovered: bool,
}

/// The resting-layer decision (rules in the module docs): which layer the
/// taskbar settles in. This drives BOTH the slide target and — via
/// [`z_layer`] — the z-band. The slide-in-flight state deliberately does
/// NOT feed this decision: a slide toward Hidden must not flip the target
/// back to the rest position (that feedback loop made the taskbar jitter
/// between two slide targets forever).
pub fn decide(inputs: &LayerInputs) -> Layer {
    if !inputs.autohide {
        // Reserve mode: the AppBar strip keeps maximized windows off the
        // taskbar, so the bar behaves like a real taskbar — except under a
        // true fullscreen window, which ignores reservations and must fully
        // hide it (a reveal gesture brings it back), or when the user asked
        // for the stricter hide-when-unhovered behavior. While the settings
        // window is open the bar stays put (rule 5): it belongs to the pill,
        // and hiding one would hide the other.
        return if (inputs.foreground_fullscreen || inputs.hide_when_unhovered)
            && !inputs.hovered
            && !inputs.menus_open
            && !inputs.settings_open
        {
            Layer::Hidden
        } else {
            Layer::Top
        };
    }
    if inputs.hovered || inputs.menus_open {
        return Layer::Top;
    }
    if inputs.settings_open {
        // Stay put (the settings window belongs to the pill and hiding one
        // would hide the other) but park beneath normal windows instead of
        // hogging the topmost band; only a covering foreground needs the
        // topmost band to keep the pill visible at all.
        return if inputs.foreground_maximized || inputs.foreground_fullscreen {
            Layer::Top
        } else {
            Layer::Desktop
        };
    }
    if inputs.foreground_fullscreen || inputs.hide_when_unhovered {
        Layer::Hidden
    } else if inputs.foreground_maximized {
        Layer::Hidden
    } else {
        Layer::Desktop
    }
}

/// The z-band layer: while a slide toward the hidden position is in flight
/// the taskbar stays Top, so it slides away over the covering window
/// instead of being clipped beneath it mid-animation.
pub fn z_layer(resting: Layer, sliding_to_hidden: bool) -> Layer {
    if sliding_to_hidden {
        Layer::Top
    } else {
        resting
    }
}

/// Whether a layer occupies the topmost band.
pub fn is_topmost(layer: Layer) -> bool {
    layer == Layer::Top
}

/// The overlay top the layer's slide should settle at.
pub fn slide_target(layer: Layer, rest_top: i32, hidden_top: i32) -> i32 {
    match layer {
        Layer::Hidden => hidden_top,
        Layer::Desktop | Layer::Top => rest_top,
    }
}

/// Owns the believed layer band and enforces it against reality.
pub struct Layering {
    /// Whether the overlay is believed to be in the topmost band.
    topmost: bool,
    bad_samples: u32,
    last_check: Instant,
}

impl Layering {
    pub fn new() -> Self {
        Self {
            topmost: true,
            bad_samples: 0,
            last_check: Instant::now(),
        }
    }

    /// The currently believed band. Feeds back into the reveal strip's
    /// geometry (renderer::reveal): the expanded hover band is active
    /// exactly while the taskbar floats.
    pub fn believed_topmost(&self) -> bool {
        self.topmost
    }

    /// Applies a decision's band to the windows when it differs from the
    /// believed one: raising lifts the overlay into the topmost band and
    /// pulls the glass surfaces directly beneath it (the DOM is never
    /// covered by its own glass mid-transition); lowering parks the whole
    /// stack just beneath the foreground window.
    pub fn apply(&mut self, overlay: HWND, state: &RendererState, layer: Layer) {
        let want = is_topmost(layer);
        if want == self.topmost {
            return;
        }
        self.topmost = want;
        state.set_desired_topmost(want);
        if want {
            raise_to_top(overlay, state);
        } else {
            lower_to_desktop(overlay, state);
        }
    }

    /// The 1 Hz sanity check: interleaved window operations (surface
    /// creation/sync on the main thread racing the raise/lower here) can
    /// leave the belief and the actual band out of step, or a glass surface
    /// stacked above the overlay — which reads as "glass without DOM". Two
    /// consecutive bad samples trigger a repair, so momentary mid-operation
    /// states don't churn.
    pub fn enforce(&mut self, overlay: HWND, state: &RendererState) {
        if self.last_check.elapsed() < Duration::from_millis(1000) {
            return;
        }
        self.last_check = Instant::now();
        let (actually_topmost, in_chain) = unsafe { measure_z(overlay, state) };
        let surfaces = state.surface_hwnd_set();
        let sane =
            actually_topmost == self.topmost && (surfaces.is_empty() || in_chain == surfaces.len());
        if sane {
            self.bad_samples = 0;
            return;
        }
        self.bad_samples += 1;
        eprintln!(
            "[taskbar] z-order desync: believed_topmost={} actual_topmost={} surfaces={} in_chain={in_chain} — repairing",
            self.topmost, actually_topmost, surfaces.len()
        );
        if self.bad_samples >= 2 {
            self.bad_samples = 0;
            // Re-apply the believed state: drift happens when another
            // thread's stacking lands between ours.
            if self.topmost {
                raise_to_top(overlay, state);
            } else {
                lower_to_desktop(overlay, state);
            }
        }
    }
}

/// Reads the overlay's actual topmost style and counts the glass surfaces
/// sitting directly beneath it in one consecutive chain — a surface
/// anywhere above the overlay covers the DOM.
unsafe fn measure_z(overlay: HWND, state: &RendererState) -> (bool, usize) {
    let style = GetWindowLongW(overlay, GWL_EXSTYLE) as u32;
    let actually_topmost = (style & WS_EX_TOPMOST.0) != 0;
    let surfaces = state.surface_hwnd_set();
    let mut in_chain = 0usize;
    let mut next = GetWindow(overlay, GW_HWNDNEXT).ok();
    while let Some(current) = next {
        if surfaces.contains(&(current.0 as isize)) {
            in_chain += 1;
            next = GetWindow(current, GW_HWNDNEXT).ok();
        } else {
            break;
        }
    }
    (actually_topmost, in_chain)
}

/// Raises the overlay into the topmost band with its glass directly
/// beneath it.
pub(crate) fn raise_to_top(overlay: HWND, state: &RendererState) {
    unsafe {
        let _ = SetWindowPos(
            overlay,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    state.set_surfaces_z_order(overlay.0 as isize);
}

/// Parks the overlay and its glass in the desktop band, just beneath the
/// foreground window.
pub(crate) fn lower_to_desktop(overlay: HWND, state: &RendererState) {
    state.lower_z_order(overlay);
}
