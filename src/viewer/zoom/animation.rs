//! The zoom tween. Every animation frame writes exactly one thing: the
//! visual scale (`zoom.current`), which the page hosts stretch their
//! existing bitmaps to. No virtualizer rescale, no geometry report, no
//! scroll write, no page write happens inside the loop — those are all
//! transaction-boundary work that the coordinator performs once, when the
//! tween lands.
//!
//! The loop reads the live `zoom.transition` signal each frame, so a
//! retarget mid-flight (a burst of `+` presses, a sidebar still sliding) is
//! adopted seamlessly: the tween continues from wherever the eye currently
//! is towards the new target, on a restarted clock, under the anchor
//! captured when the transaction opened.

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

pub(crate) fn ease_out_cubic(t: f64) -> f64 {
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
                // Landing without a tween: present the target, commit once.
                state.viewer.zoom.current.set(t.to);
                finish_transition(&state, &engine, &t);
                alive.set(false);
                return;
            }
            let x = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            // The ONE per-frame write: the visual scale. Page hosts stretch
            // their bitmaps to it; the virtualizer geometry stays put.
            state.viewer.zoom.current.set(t.from + (t.to - t.from) * ease_out_cubic(x));
            if x >= 1.0 {
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
