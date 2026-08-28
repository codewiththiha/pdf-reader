//! [`ZoomController`]: the single authority for *what* the zoom is and how its
//! animation runs.
//!
//! The controller owns the one write path for the layout scale. Manual zoom
//! goes through `zoom_to` (which clears the fit mode so fit cannot fight the
//! gesture); fit recomputes a target through `set_fit` / `apply_fit` and hands
//! it back to the same path. Nobody outside this module writes `zoom.layout`
//! or asks the engine to rescale, so a gesture and a refit can no longer race
//! along separate code paths.
//!
//! The previous coordinator scattered that intent across `request_zoom`,
//! `zoom_system`, `commit_scale`, and two thread-local flags
//! (`gesture_owns_layout`, `take_commit_echo`). The flag that is now gone is
//! replaced by reading the real `zoom_animating` signal.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::math::clamp_scale;
use pdf_core::math::FitMode;

use crate::state::ReaderState;
use crate::viewer::engine::ViewerEngine;
use crate::components::primitives::motion::reduced_motion::prefers_reduced_motion;

/// rAF step that can re-arm itself.
type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Duration of the zoom layout animation.
const ZOOM_ANIM_MS: f64 = 200.0;

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// The one zoom authority. `Copy` so it can be handed out by value; it holds
/// no per-instance state beyond the engine reference and the (already
/// exclusive) animation slot.
#[derive(Clone)]
pub struct ZoomController {
    engine: ViewerEngine,
}

impl ZoomController {
    pub fn new(engine: ViewerEngine) -> Self {
        Self { engine }
    }

    /// A manual zoom. Clears the fit mode so a simultaneous `fit == None`
    /// derivation cannot pull the layout back toward a fit value, then posts
    /// the gesture through the animation loop.
    pub fn zoom_to(&self, state: ReaderState, target: f64, animate: bool) {
        let target = clamp_scale(target);
        state.viewer.zoom.requested.set(target);
        state.viewer.fit.set(FitMode::None);
        self.post(state, target, animate);
    }

    fn post(&self, state: ReaderState, target: f64, animate: bool) {
        let token = ZOOM_TOKEN.with(|t| {
            let next = t.get() + 1;
            t.set(next);
            next
        });
        state
            .viewer
            .zoom
            .request
            .set(Some((target, animate, token)));
    }

    /// Starts the animation watcher. Called once from the reader shell.
    pub fn drive(&self, state: ReaderState) {
        let engine = self.engine.clone();

        // While a scale animation is in flight, the virtualizer's DOM scroll
        // echo must not overwrite the core anchor (the browser fires those
        // per-frame scroll events one frame late, so the echo is stale).
        {
            let v = engine.vertical.clone();
            let hv = engine.horizontal.clone();
            Effect::new(move |_| {
                if state.viewer.zoom_animating.get() {
                    v.suspend_scroll_feedback();
                    hv.suspend_scroll_feedback();
                } else {
                    v.resume_scroll_feedback();
                    hv.resume_scroll_feedback();
                }
            });
        }

        let anim_slot = StoredValue::new_local(None::<StepSlot>);
        let live_token = StoredValue::new_local(Rc::new(Cell::new(0u64)));

        Effect::new(move |_| {
            let Some((target, animate, token)) = state.viewer.zoom.request.get() else {
                return;
            };
            let live = live_token.get_value();
            live.set(token);

            let from = state.viewer.zoom.layout.get_untracked();
            if (target - from).abs() < 1e-9 {
                commit_scale(state, target);
                engine.vertical.resume_measurements();
                engine.horizontal.resume_measurements();
                return;
            }

            engine.vertical.suspend_measurements();
            engine.horizontal.suspend_measurements();

            let commit = {
                let engine = engine.clone();
                move |final_scale: f64| {
                    commit_scale(state, final_scale);
                    engine.vertical.resume_measurements();
                    engine.horizontal.resume_measurements();
                }
            };

            if !animate || prefers_reduced_motion() {
                state.viewer.zoom_animating.set(true);
                engine.relayout_scale(&state, target / from);
                commit(target);
                return;
            }

            state.viewer.zoom_animating.set(true);

            let start = js_sys::Date::now();
            let step_slot: StepSlot = Rc::new(RefCell::new(None));
            let step_self = Rc::downgrade(&step_slot);
            let live_step = live.clone();
            let step_engine = engine.clone();
            let step: Rc<dyn Fn()> = Rc::new(move || {
                if live_step.get() != token {
                    return;
                }
                let t = ((js_sys::Date::now() - start) / ZOOM_ANIM_MS).clamp(0.0, 1.0);
                let eased = ease_out_cubic(t);
                let want = from + (target - from) * eased;
                let cur = state.viewer.zoom.layout.get_untracked();

                step_engine.relayout_scale(&state, want / cur);
                state.viewer.zoom.layout.set(want);

                if t >= 1.0 {
                    commit(target);
                    return;
                }
                if let Some(next) = step_self.upgrade().and_then(|slot| slot.borrow().clone()) {
                    request_animation_frame(move || next());
                }
            });
            *step_slot.borrow_mut() = Some(step.clone());
            anim_slot.set_value(Some(step_slot));
            request_animation_frame(move || step());
        });
    }

}

thread_local! {
    static ZOOM_TOKEN: Cell<u64> = const { Cell::new(0) };
    static CUR_ZOOM: RefCell<Option<ZoomController>> = const { RefCell::new(None) };
}

/// Register the controller the reader shell created, so the free-function
/// entry points (zoom controls, keyboard shortcuts) route through it.
pub fn register(controller: &ZoomController) {
    CUR_ZOOM.with(|c| *c.borrow_mut() = Some(controller.clone()));
}

/// Free-function entry for a manual zoom. Clears the fit mode (via
/// `ZoomController::zoom_to`) so a simultaneous `fit == None` derivation
/// cannot pull the layout back toward a fit value.
pub fn request_zoom(state: ReaderState, target: f64, animate: bool) {
    if let Some(z) = CUR_ZOOM.with(|c| c.borrow().clone()) {
        z.zoom_to(state, target, animate);
    }
}

pub fn commit_scale(state: ReaderState, scale: f64) {
    state.viewer.zoom.layout.set(scale);
    state.viewer.zoom.level.set(scale);
    state.viewer.zoom.render.set(scale);
    state.viewer.zoom_animating.set(false);
    pdf_engine::api::sweep();
}
