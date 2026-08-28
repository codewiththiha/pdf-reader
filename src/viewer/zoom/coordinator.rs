//! [`ZoomController`]: the single authority for what the effective zoom is
//! and how every change of it runs.
//!
//! All zoom traffic arrives as commands on `viewer.zoom.commands` — toolbar
//! buttons, keyboard steps, the fit watcher, the resize watcher — and is
//! executed by exactly one effect here. The controller:
//!
//! 1. resolves the command to a target (fit and constraint maths live in
//!    super::target),
//! 2. captures the ONE document focus and the stage pivot once per
//!    transaction (super::anchor) — a chained or retargeted transition
//!    reuses them, never a mid-flight recapture,
//! 3. tweens the PRESENTATION RATIO only (super::animation): one linear CSS
//!    transform scales the whole document surface; the virtualizer does not
//!    participate in the visual zoom at all,
//! 4. and on landing commits the geometry exactly once, restores the focus,
//!    and releases the freezes (render suspension, page/scroll
//!    synchronisation, geometry feedback, scroll echo).
//!
//! Around the commit, the strips' zombie retention grace is raised so pages
//! evicted by the geometry change keep their DOM (and their last bitmap)
//! briefly — the bridge that makes the commit invisible.
//!
//! Because the controller is created with the reader page and lives exactly
//! as long as its reactive owner, there is no global registry to leak or
//! race: the old thread-local `CUR_ZOOM` registration is gone, and the
//! free-function `request_zoom` entry points with it.

use std::time::Duration;

use leptos::prelude::*;

use crate::state::reader::{ZoomTransition, ReaderState};
use crate::viewer::engine::ViewerEngine;

use super::anchor;
use super::animation::Tween;
use super::config;
use super::target;

/// Scales closer than this are the same scale: a refit that lands within a
/// twentieth of a percent of the committed scale is not worth a transition
/// (and not worth a re-render).
const SETTLED_EPSILON: f64 = 0.0005;

/// The one zoom authority. `Clone` so it can be handed out by value; it
/// holds nothing but the engine reference.
#[derive(Clone)]
pub struct ZoomController {
    engine: ViewerEngine,
}

impl ZoomController {
    pub fn new(engine: ViewerEngine) -> Self {
        Self { engine }
    }

    /// Start the command consumer and the freeze bookkeeping. Called once
    /// from the reader shell.
    pub fn drive(&self, state: ReaderState) {
        let engine = self.engine.clone();

        // While a transition is in flight, three feedback loops must stand
        // down: the browser's per-frame scroll echo (stale by one frame —
        // adopting it would fight the anchor), size reports from the strips
        // (the guard in `PageStrip` already refuses to report mid-zoom; the
        // suspension makes that airtight at the virtualizer too), and — on
        // landing — the flush order, because `commit_geometry` must finish
        // before buffered measurements re-enter.
        {
            let v = engine.vertical.clone();
            let hv = engine.horizontal.clone();
            Effect::new(move |_| {
                if state.viewer.zoom.transition.get().is_some() {
                    v.suspend_scroll_feedback();
                    hv.suspend_scroll_feedback();
                    v.suspend_measurements();
                    hv.suspend_measurements();
                } else {
                    v.resume_scroll_feedback();
                    hv.resume_scroll_feedback();
                    v.resume_measurements();
                    hv.resume_measurements();
                }
            });
        }

        let tween = Tween::new();

        Effect::new(move |_| {
            let Some((cmd, animate, _token)) = state.viewer.zoom.commands.get() else {
                return;
            };
            // Resolve against the in-flight target so chained steps advance
            // one preset per press. All reads inside are untracked: this
            // effect subscribes to the command signal and nothing else.
            let Some(target) = target::resolve(&state, cmd, state.viewer.zoom.in_flight_target())
            else {
                return;
            };

            let zoom = state.viewer.zoom;
            let committed = zoom.committed.get_untracked();
            let in_flight = zoom.transition.get_untracked();
            let settled = in_flight.map(|t| t.to).unwrap_or(committed);
            if (target - settled).abs() < SETTLED_EPSILON {
                // Already there (or already heading there): nothing to move.
                return;
            }
            // The focus and the stage pivot are captured at transaction
            // OPEN. A retarget keeps the original logical position — the
            // reader's eyes stay where they were when the gesture began.
            let mode = state.viewer.mode.get_untracked();
            let (focus, origin) = in_flight
                .map(|t| (t.focus, t.origin))
                .unwrap_or_else(|| (anchor::capture_focus(&engine, &state), anchor::stage_origin(&engine, &state, mode)));
            // `from` is the visual scale RIGHT NOW: the presentation ratio
            // times the committed scale, so a retarget continues from
            // wherever the eye currently is instead of teleporting.
            let from = zoom.committed.get_untracked() * zoom.presentation.get_untracked();
            zoom.transition.set(Some(ZoomTransition {
                from,
                to: target,
                start_ms: js_sys::Date::now(),
                animate,
                focus,
                origin,
            }));
            // Bridge the commit before it happens: raise the strips' zombie
            // grace so pages the geometry change evicts keep their DOM past
            // the animation's end.
            let retention = config::profile_for(mode).retention;
            engine.vertical.set_retention_grace(retention.grace_ms);
            engine.horizontal.set_retention_grace(retention.grace_ms);
            tween.arm(state, engine.clone());
        });
    }
}

/// Land a transition: one geometry commit at the target, the focus put
/// back, the freezes released. Runs from the tween loop's final frame (or
/// its first frame, for an untweened landing).
pub(crate) fn finish_transition(state: &ReaderState, engine: &ViewerEngine, t: &ZoomTransition) {
    // Geometry first, against the still-suspended virtualizers: sizes, one
    // rescale per strip, the focus restored on the new layout (the restore
    // itself is one explicit synchronisation step inside the commit).
    engine.commit_geometry(state, t.to, &t.focus);
    // Then the scales. The presentation ratio drops back to 1.0 in the same
    // flush that installs the target geometry and stretches the mounted
    // hosts to it, so the transform is replaced by real geometry at the
    // same visual size — the commit is imperceptible by construction.
    state.viewer.zoom.committed.set(t.to);
    state.viewer.zoom.presentation.set(1.0);
    // Releasing the transition last is what un-freezes page/scroll sync and
    // geometry feedback — everything downstream re-runs against the new
    // committed geometry, never against a half-landed one.
    state.viewer.zoom.transition.set(None);
    // Nothing renders inside a transaction; sweep the rasters now that the
    // commit pass has issued its renders.
    pdf_engine::api::sweep();
    // The zombies that shielded this commit keep their raised grace until
    // it expires; only the GRACE resets here (their expiry was fixed when
    // they were evicted). Guarded so a new transaction that opened in the
    // meantime is not stepped on — its own finish will reschedule.
    let grace = config::profile_for(state.viewer.mode.get_untracked()).retention.grace_ms;
    let v = engine.vertical.clone();
    let hv = engine.horizontal.clone();
    let zoom = state.viewer.zoom;
    let _ = set_timeout_with_handle(
        move || {
            if zoom.transition.get_untracked().is_none() {
                v.reset_retention_grace();
                hv.reset_retention_grace();
            }
        },
        Duration::from_millis(grace as u64),
    );
}
