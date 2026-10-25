//! The auto-hide slide animator, in one place.
//!
//! Owns the eased motion between the taskbar's rest and hidden positions:
//! the retarget detection (interrupting a slide reverses smoothly from
//! wherever the pill currently is), the eased progress integration, and the
//! choreography that rides along — the pill glass's wake/park, the settings
//! window (which belongs to the taskbar), the DOM's `taskbar-slide` event
//! and the reveal-time glass scale.
//!
//! The side effects run through the renderer handle, exactly like the
//! z-order band ([`super::layering`]) applies its decisions; the retarget
//! rule consumes the resting layer the loop decided
//! ([`super::layering::slide_target`]), so the slide never fights the
//! layering rules.

use std::time::Instant;
use tauri::{Emitter, Manager};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    },
};

use super::RendererState;

/// One full slide lasts this long; the easing direction shapes the ends.
const SLIDE_DURATION_SECS: f32 = 0.14;

/// One tick's geometry inputs for the slide animator.
pub struct SlideInputs {
    /// The overlay window's left edge (the slide moves only in Y).
    pub window_left: i32,
    /// Where the overlay's top edge is right now.
    pub current_top: i32,
    /// Where this tick's resting layer wants the top edge.
    pub target_top: i32,
    /// The primary monitor's height: the hidden position parks the top edge
    /// at (or below) it.
    pub screen_height: i32,
    /// How far above the screen's bottom edge the cursor is, for the reveal
    /// log.
    pub cursor_above_bottom: i32,
    /// The reveal strip's current band height, for the reveal log.
    pub strip_band_px: i32,
    /// Whether the taskbar floats in the topmost band, for the reveal log.
    pub floating: bool,
}

/// Owns the eased slide state between the rest and hidden positions and
/// applies it to the overlay window every tick.
pub struct Slide {
    /// (progress, from_top, to_top, last_applied): the eased progress
    /// between the from/to tops, and the top actually applied to the window
    /// last (steps only fire when it moves).
    state: Option<(f32, i32, i32, i32)>,
    last_tick: Instant,
}

impl Slide {
    pub fn new() -> Self {
        Self {
            state: None,
            last_tick: Instant::now(),
        }
    }

    /// Whether a slide toward the hidden position is in flight. Read both
    /// before the tick (a hide in flight suppresses the interactive hold in
    /// the reveal trigger) and after it (the z-band stays topmost while the
    /// bar slides away OVER the covering window).
    pub fn hiding(&self, screen_height: i32) -> bool {
        matches!(&self.state, Some((_, _, to, _)) if *to >= screen_height)
    }

    /// Drives one tick: retargets when the resting layer's target moved
    /// (restarting from wherever the pill currently is, so interrupting a
    /// slide reverses smoothly), integrates the eased progress with the REAL
    /// inter-tick interval, applies the new top edge to the overlay window,
    /// translates the glass surfaces by the same step, and parks/wakes the
    /// pill glass and the settings window at the slide's ends.
    pub fn tick(&mut self, renderer: &tauri::AppHandle, hwnd: HWND, tick: SlideInputs) {
        let state = renderer.state::<RendererState>();
        let dt = Instant::now().duration_since(self.last_tick).as_secs_f32();
        self.last_tick = Instant::now();
        let mismatch = match &self.state {
            Some((_, _, to, _)) => *to != tick.target_top,
            None => (tick.current_top - tick.target_top).abs() > 1,
        };
        if mismatch {
            // (Re)start from wherever the pill currently is, so interrupting
            // a slide reverses smoothly.
            self.state = Some((0.0, tick.current_top, tick.target_top, tick.current_top));
            state.set_sliding(true);
            let revealing = tick.target_top < tick.current_top;
            if revealing {
                eprintln!(
                    "[reveal] triggered at {}px above the bottom (band={}px, floating={})",
                    tick.cursor_above_bottom, tick.strip_band_px, tick.floating
                );
                // The pill's glass was parked while the taskbar was hidden;
                // wake it before the slide so it renders through the whole
                // animation.
                state.set_pill_hidden(false);
                // The settings window belongs to the taskbar: it comes back
                // whenever the pill does.
                if let Some(settings) = renderer.get_webview_window("settings") {
                    let _ = settings.show();
                }
            }
            let _ = renderer.emit("taskbar-slide", revealing);
        }
        if let Some((progress, from_top, to_top, last_applied)) = &mut self.state {
            *progress = (*progress + dt / SLIDE_DURATION_SECS).min(1.0);
            let eased = if *to_top < *from_top {
                // Revealing (moving up): ease-out cubic — fast start,
                // settles gently into place.
                1.0 - (1.0 - *progress) * (1.0 - *progress) * (1.0 - *progress)
            } else {
                // Hiding (moving down): ease-in cubic — leaves gently,
                // accelerates away.
                *progress * *progress * *progress
            };
            let new_top = *from_top + (((*to_top - *from_top) as f32) * eased).round() as i32;
            let dy = new_top - *last_applied;
            if dy != 0 {
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        tick.window_left,
                        new_top,
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                    );
                }
                state.translate_surfaces(dy);
                *last_applied = new_top;
            }
            // The glass scales in with the reveal only (starting ~85%
            // small); while hiding it stays at full size and just slides
            // away.
            if *to_top < *from_top {
                state.set_pill_reveal_scale(0.85 + 0.15 * eased);
            }
            if *progress >= 1.0 {
                let finished_hidden = *to_top >= tick.screen_height;
                self.state = None;
                // Re-enable full surface syncs once the slide is over (the
                // final Moved event lands after this, which is fine — the
                // geometry already agrees).
                state.set_sliding(false);
                // Fully hidden: the pill's glass is off-screen, so park it
                // (the shared capture then skips its full-screen copies
                // too), and take the settings window with the taskbar.
                if finished_hidden {
                    state.set_pill_hidden(true);
                    if let Some(settings) = renderer.get_webview_window("settings") {
                        let _ = settings.hide();
                    }
                }
            }
        }
    }
}
