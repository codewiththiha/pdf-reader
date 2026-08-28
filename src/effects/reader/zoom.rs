//! The zoom coordinator: a zoom is a layout animation of bitmaps we already
//! painted, followed by one crisp re-render. `request_zoom` posts
//! `(target, animate, token)`; `zoom_system` drives `zoom.layout` on rAF,
//! asks the viewer engine to rescale/anchor the strip each frame, and commits
//! once the gesture settles.
//!
//! The relayout itself lives in [`crate::viewer::engine`] (the single geometry
//! owner); this module owns the *intent* and the animation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::math::clamp_scale;

use crate::state::ReaderState;
use crate::viewer::engine::ViewerEngine;

/// rAF step that can re-arm itself. StoredValue already wraps a RefCell, so we
/// only need one extra Rc for the Weak self-reference the loop upgrades.
type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Duration of the zoom layout animation. Long enough to read as motion, short
/// enough that the crisp render feels immediate.
const ZOOM_ANIM_MS: f64 = 200.0;

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

pub fn request_zoom(state: ReaderState, target: f64, animate: bool) {
    let target = clamp_scale(target);
    state.viewer.zoom.requested.set(target);
    set_gesture_owns_layout(true);
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

thread_local! {
    static ZOOM_TOKEN: Cell<u64> = const { Cell::new(0) };
    static ZOOM_GESTURE: Cell<bool> = const { Cell::new(false) };
    static COMMIT_ECHO: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn gesture_owns_layout() -> bool {
    ZOOM_GESTURE.with(|g| g.get())
}

fn set_gesture_owns_layout(owns: bool) {
    ZOOM_GESTURE.with(|g| g.set(owns));
}

pub(super) fn take_commit_echo() -> bool {
    COMMIT_ECHO.with(|c| c.replace(false))
}

pub fn zoom_system(state: ReaderState, engine: ViewerEngine) {
    // While any scale animation is in flight (a zoom gesture, a sidebar slide,
    // a window-resize drag — all raise `zoom_animating` and all end in
    // `commit_scale`, which clears it), the virtualizer's DOM scroll echo must
    // not overwrite the core anchor: the browser fires those per-frame scroll
    // events one frame late, so the echo is stale and the next anchored
    // rescale pins from it, making the content oscillate instead of gliding.
    // The echo is re-adopted the moment the animation commits.
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

pub(super) fn commit_scale(state: ReaderState, scale: f64) {
    set_gesture_owns_layout(false);
    COMMIT_ECHO.with(|c| c.set(true));
    state.viewer.zoom.layout.set(scale);
    state.viewer.zoom.level.set(scale);
    state.viewer.zoom.render.set(scale);
    state.viewer.zoom_animating.set(false);
    // The scale is settled and the pages have (or will) be re-rendered at it:
    // rasters of every other scale are dead weight, so let the engine drop
    // them now instead of waiting for the next idle sweep.
    pdf_engine::api::sweep();
}
