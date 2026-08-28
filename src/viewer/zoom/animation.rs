//! The zoom tween: deliberately the most boring animation in the reader.
//!
//! Every animation frame writes exactly ONE thing — the presentation ratio
//! (`zoom.presentation`), which scales the whole document surface through a
//! single CSS transform on the zoom stage. Pages, gaps and edges move
//! together as one continuous surface; no page's layout box changes, the
//! virtualizer's geometry stays at the committed scale, nothing measures,
//! nothing renders, nothing mounts or unmounts.
//!
//! The interpolation is LINEAR, on purpose. The whole point of the stage is
//! that the final geometry commit is visually imperceptible: the reader has
//! just watched the surface scale at a constant rate to exactly the target
//! scale, and the commit swaps the transform for real geometry at that same
//! scale. An eased tween decelerates right before that swap, so the eye
//! catches the seam; a constant-velocity resize keeps the landing hidden.
//! No springs, no overshoot, no stagger — the reader zooms a piece of paper,
//! not a UI element.
//!
//! The loop reads the live `zoom.transition` signal each frame, so a
//! retarget mid-flight (a burst of `+` presses, a sidebar still sliding) is
//! adopted seamlessly: the tween continues from wherever the eye currently
//! is towards the new target, on a restarted clock, under the focus
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
                // Landing without a tween: the commit is the whole story.
                finish_transition(&state, &engine, &t);
                alive.set(false);
                return;
            }
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            // The ONE per-frame write: the presentation ratio. Linear — see
            // the module docs for why the commit seam must not be eased.
            let visual = t.from + (t.to - t.from) * progress;
            let committed = state.viewer.zoom.committed.get_untracked();
            state.viewer.zoom.presentation.set(visual / committed);
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
