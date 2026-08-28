//! The zoom tween.
//!
//! Every animation frame works out the scale the eye should be at and shows
//! it — but HOW it is shown depends on the view mode, because the two scroll
//! modes are laid out by fundamentally different machinery:
//!
//! - HORIZONTAL relayouts. Each frame hands the engine the ratio the display
//!   scale just moved through, and the engine rescales the strip so the
//!   layout genuinely follows. The virtualizer's rescale anchor holds the
//!   reader's view steady while the sizes underneath move, so there is
//!   nothing to capture before the gesture and nothing to restore after it.
//! - VERTICAL and the PAGINATED modes transform. Each frame writes one CSS
//!   `scale()` on the strip's content surface (`zoom.presentation`), so the
//!   whole document resizes as one continuous surface: no page's layout box
//!   moves, no gap opens, the virtualizer's geometry stays put and nothing
//!   measures. Fighting that with per-frame relayouts is what made vertical
//!   scrolling feel like it was fighting the browser's own flow.
//!
//! Both write the same `zoom.display`, so everything that reads "the scale
//! the reader is looking at" — the readout, the fit maths, the overlays —
//! is agnostic to which path the zoom took.
//!
//! The interpolation is an out-cubic, which is what the smooth zoom has
//! always felt like: it covers ground early and decelerates into the target
//! instead of stopping dead on it.
//!
//! The loop reads the live `zoom.transition` signal each frame, so a
//! retarget mid-flight (a burst of `+` presses, a sidebar still sliding) is
//! adopted seamlessly: the tween continues from wherever the eye currently
//! is towards the new target, on a restarted clock.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;

use crate::components::primitives::motion::reduced_motion::prefers_reduced_motion;
use crate::state::reader::ReaderState;
use crate::viewer::engine::ViewerEngine;

use super::config;
use super::coordinator::finish_transition;

/// rAF step that can re-arm itself.
type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The tween's progress curve: covers ground early, decelerates onto the
/// target scale instead of stopping dead on it.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// The single tween loop owned by the zoom controller.
///
/// The slot is stored here — not in a local of `arm` — so the running step
/// closure keeps itself alive across frames (each step hands the next frame
/// the `Rc` it clones out of the slot). `arm` is idempotent: while a loop
/// is alive it adopts whatever transition is on the signal, so retargets
/// never stack a second loop.
pub(crate) struct Tween {
    alive: Rc<Cell<bool>>,
    slot: StepSlot,
}

impl Tween {
    pub(crate) fn new() -> Self {
        Self {
            alive: Rc::new(Cell::new(false)),
            slot: Rc::new(RefCell::new(None)),
        }
    }

    /// Ensure a loop is running for the current transition.
    pub(crate) fn arm(&self, state: ReaderState, engine: ViewerEngine) {
        if self.alive.get() {
            return; // the live loop reads the signal; it will pick this up
        }
        self.alive.set(true);

        let alive = self.alive.clone();
        let weak = Rc::downgrade(&self.slot);
        let step: Rc<dyn Fn()> = Rc::new(move || {
            // Idle? The loop dies here until the next `arm`.
            let Some(t) = state.viewer.zoom.transition.get_untracked() else {
                alive.set(false);
                return;
            };
            let mode = state.viewer.mode.get_untracked();
            let is_horizontal = mode == ViewMode::ScrollHorizontal;
            let duration = config::profile_for(mode).duration_ms();
            if !t.animate || duration <= 0.0 || prefers_reduced_motion() {
                // Landing without a tween. The horizontal strip still needs
                // its layout moved to the target — there is no transform to
                // carry it — so it gets one relayout; the commit does the
                // same job for the transform-scaled modes.
                if is_horizontal {
                    let from = state.viewer.zoom.display.get_untracked();
                    engine.relayout_to(&state, t.to / from);
                    state.viewer.zoom.display.set(t.to);
                }
                finish_transition(&state, &engine, &t);
                alive.set(false);
                return;
            }
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);

            if is_horizontal {
                // The per-frame pair: relay the layout out by the ratio the
                // display scale is about to move through, then show it. The
                // engine reads `display` to work out the strip's exact
                // widths, so the relayout must come first.
                let cur = state.viewer.zoom.display.get_untracked();
                engine.relayout_to(&state, visual / cur);
                state.viewer.zoom.display.set(visual);
            } else {
                // One CSS transform on the content surface. `display` still
                // moves so every reader of the live scale agrees, but no
                // geometry does — the commit installs it at the end.
                let committed = state.viewer.zoom.committed.get_untracked();
                state.viewer.zoom.presentation.set(visual / committed);
                state.viewer.zoom.display.set(visual);
            }
            if progress >= 1.0 {
                finish_transition(&state, &engine, &t);
                alive.set(false);
                return;
            }
            if let Some(next) = weak.upgrade().and_then(|s| s.borrow().clone()) {
                request_animation_frame(move || next());
            }
        });
        *self.slot.borrow_mut() = Some(step.clone());
        request_animation_frame(move || step());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_starts_fast_and_lands_exactly() {
        assert!(ease_out_cubic(0.1) > 0.27); // out-cubic covers ground early
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-12);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        // Out-of-range inputs must not overshoot the endpoints.
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
    }
}
