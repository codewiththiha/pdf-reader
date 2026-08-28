//! The zoom tween.
//!
//! Every animation frame does exactly one thing: work out the scale the eye
//! should be at, and hand the engine the ratio between that scale and the
//! scale the layout currently has. The engine relays the strips out and the
//! page hosts stretch their bitmaps to the new size, so the document resizes
//! continuously — the reader watches the paper itself change size, with the
//! virtualizer anchoring the view as the sizes underneath move.
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
            let duration = config::profile_for(state.viewer.mode.get_untracked()).duration_ms();
            if !t.animate || duration <= 0.0 || prefers_reduced_motion() {
                // Landing without a tween: one relayout to the target, then
                // the commit.
                let from = state.viewer.zoom.display.get_untracked();
                engine.relayout_to(&state, t.to / from);
                finish_transition(&state, &engine, &t);
                alive.set(false);
                return;
            }
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            // The per-frame pair: relay the layout out by the ratio the
            // display scale is about to move through, then show it. The
            // engine reads `display` to work out the horizontal strip's
            // exact widths, so the relayout must come first.
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);
            let cur = state.viewer.zoom.display.get_untracked();
            engine.relayout_to(&state, visual / cur);
            state.viewer.zoom.display.set(visual);
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
