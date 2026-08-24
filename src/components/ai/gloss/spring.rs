//! The spring, as a Leptos effect. Springs `value` toward `target`; while
//! `snap` is true (dragging / processing bloom / reduced-motion) it jumps
//! instead of wobbling.
//!
//! Mirrors the self-referencing rAF loop in `effects/reader/zoom.rs`: one
//! `StepSlot` (`Rc<RefCell<Option<Rc<dyn Fn()>>>>`) owns the step closure, the
//! closure holds a *weak* ref back to it to re-arm, and replacing the slot
//! (when `target` changes) drops the old loop's only strong ref so it dies.
//! `vel` and `last_ms` live outside the Effect so velocity survives a
//! retarget, keeping the morph continuous.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::gloss::{boxes_close, step_spring, GlossBox};

type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The largest field magnitude that counts as "stopped" for loop teardown.
const SETTLE_EPS: f64 = 0.6;

fn velocity_small(v: GlossBox) -> bool {
    v.x.abs() < SETTLE_EPS
        && v.y.abs() < SETTLE_EPS
        && v.w.abs() < SETTLE_EPS
        && v.h.abs() < SETTLE_EPS
        && v.r.abs() < SETTLE_EPS
}

/// Springs `value` toward `target`. Setting `target` to `None` clears the
/// value and stops the loop. Returns the live box signal (`None` while clear).
pub fn use_spring_box(target: Signal<Option<GlossBox>>, snap: Signal<bool>) -> RwSignal<Option<GlossBox>> {
    let value = RwSignal::new(target.get_untracked());
    let vel = StoredValue::new_local(GlossBox::default());
    let last_ms = StoredValue::new_local(f64::NAN);
    // The owner-scoped holder for the current step closure, so it outlives the
    // rAF callbacks between Effect re-runs (same shape as zoom.rs's anim_slot).
    let anim_slot = StoredValue::new_local(None::<StepSlot>);

    Effect::new(move |_| {
        // A new target. Reading it here both tracks it (so this re-runs) and
        // gates the run: no target means clear and stop.
        if target.get().is_none() {
            value.set(None);
            vel.set_value(GlossBox::default());
            last_ms.set_value(f64::NAN);
            return;
        }

        let slot: StepSlot = Rc::new(RefCell::new(None));
        let weak = Rc::downgrade(&slot);

        let step: Rc<dyn Fn()> = Rc::new(move || {
            let dest = match target.get_untracked() {
                Some(d) => d,
                // Target cleared mid-flight: stop.
                None => return,
            };

            let now = js_sys::Date::now();
            let mut last = last_ms.get_value();
            if last.is_nan() {
                last = now;
            }
            // Clamp long frames to the integrator's stability bound.
            let dt = ((now - last) / 1000.0).min(0.032);
            last_ms.set_value(now);

            if snap.get_untracked() {
                // No wobble while dragging or during the processing beat.
                vel.set_value(GlossBox::default());
                let already = value.get_untracked().is_some_and(|v| boxes_close(v, dest, 0.25));
                if !already {
                    value.set(Some(dest));
                }
                // Snapped to target; target changes re-run the Effect to move
                // again, so no further frames are needed here.
                return;
            }

            let cur = value.get_untracked().unwrap_or(dest);
            let (next, next_vel) = step_spring(cur, vel.get_value(), dest, dt);
            vel.set_value(next_vel);

            // Settled: park exactly on the target and stop scheduling.
            if boxes_close(next, dest, SETTLE_EPS) && velocity_small(next_vel) {
                value.set(Some(dest));
                vel.set_value(GlossBox::default());
                return;
            }

            value.set(Some(next));

            // Re-arm: upgrade our weak slot to the strong closure and queue it.
            if let Some(next) = weak.upgrade().and_then(|s| s.borrow().clone()) {
                request_animation_frame(move || next());
            }
        });

        *slot.borrow_mut() = Some(step.clone());
        anim_slot.set_value(Some(slot));
        request_animation_frame(move || step());
    });

    value
}
