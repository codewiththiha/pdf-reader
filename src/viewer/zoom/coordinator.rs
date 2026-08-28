//! [`ZoomController`]: the single authority for what the effective zoom is
//! and how every change of it runs.
//!
//! All zoom traffic arrives as commands on `viewer.zoom.commands` — toolbar
//! buttons, keyboard steps, the fit watcher, the resize watcher — and is
//! executed by exactly one effect here. The controller:
//!
//! 1. resolves the command to a target (fit and constraint maths live in
//!    super::target),
//! 2. opens a transition from the scale on screen right now to that target,
//!    capturing the document focus and the stage pivot for the modes that
//!    need them (super::anchor),
//! 3. tweens the live display scale (super::animation) — relaying the
//!    horizontal strip's layout out through the engine on every frame, and
//!    scaling the other modes' content surface through one CSS transform,
//! 4. and on landing commits the geometry and the render scale and releases
//!    the freezes (render suspension, page/scroll synchronisation, geometry
//!    feedback, scroll echo).
//!
//! Which of the two visual paths a zoom takes is the view mode's call, not
//! the controller's: see `crate::viewer` for why each mode picked the one it
//! did.
//!
//! Around the transaction, the strips' zombie retention grace is raised so
//! pages a moving window evicts keep their DOM (and their last bitmap)
//! briefly — the bridge that keeps the zoom from popping pages out.
//!
//! Because the controller is created with the reader page and lives exactly
//! as long as its reactive owner, there is no global registry to leak or
//! race: the old thread-local `CUR_ZOOM` registration is gone, and the
//! free-function `request_zoom` entry points with it.

use std::time::Duration;

use leptos::prelude::*;
use pdf_core::layout::ViewMode;

use crate::state::reader::{ReaderState, ZoomTransition};
use crate::viewer::engine::ViewerEngine;

use super::anchor;
use super::animation::Tween;
use super::config;
use super::target;

/// Scales closer than this are the same scale: a refit that lands within a
/// twentieth of a percent of the settled scale is not worth a transition
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
        // adopting it would fight the rescale anchor), size reports from the
        // strips (the guard in `PageStrip` already refuses to report
        // mid-zoom; the suspension makes that airtight at the virtualizer
        // too), and — on landing — the flush order, because the last
        // relayout must finish before buffered measurements re-enter.
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
            let display = zoom.display.get_untracked();
            let in_flight = zoom.transition.get_untracked();
            let settled = in_flight.map(|t| t.to).unwrap_or(display);
            if (target - settled).abs() < SETTLED_EPSILON {
                // Already there (or already heading there): nothing to move.
                return;
            }
            // The focus and the stage pivot are captured at transaction OPEN
            // for the modes that scale through a CSS transform — a retarget
            // keeps the original logical position, so the reader's eyes stay
            // where they were when the gesture began. The horizontal strip
            // relayouts continuously and never reads either one.
            let mode = state.viewer.mode.get_untracked();
            let (focus, origin) = in_flight
                .map(|t| (t.focus, t.origin))
                .unwrap_or_else(|| {
                    (
                        anchor::capture_focus(&engine, &state),
                        anchor::stage_origin(&engine, &state, mode),
                    )
                });
            // `from` is the visual scale RIGHT NOW, so a retarget continues
            // from wherever the eye currently is instead of teleporting.
            // `display` carries that in both designs: the transform-scaled
            // modes keep it in lock-step with `committed × presentation`,
            // and for the horizontal strip it is the only value that is true
            // mid-flight — reading `committed` there would snap a chained
            // `+ +` back to the last commit.
            zoom.transition.set(Some(ZoomTransition {
                from: display,
                to: target,
                start_ms: js_sys::Date::now(),
                animate,
                focus,
                origin,
            }));
            // Bridge the relayouts before they happen: raise the strips'
            // zombie grace so pages the moving window evicts keep their DOM
            // past the animation's end.
            let retention = config::profile_for(mode).retention;
            engine.vertical.set_retention_grace(retention.grace_ms);
            engine.horizontal.set_retention_grace(retention.grace_ms);
            tween.arm(state, engine.clone());
        });
    }
}

/// Land a transition: commit the geometry where the view mode needs it, bring
/// every scale onto the target, and release the freezes.
///
/// The horizontal strip has already been relaid out frame by frame, so there
/// is nothing left to do but settle the scales. The transform-scaled modes
/// have been showing the target through a CSS transform over untouched
/// geometry, so the commit is the one moment their layout, rasters and scroll
/// move — at the same visual size the transform was already showing.
pub(crate) fn finish_transition(state: &ReaderState, engine: &ViewerEngine, t: &ZoomTransition) {
    let mode = state.viewer.mode.get_untracked();
    let is_horizontal = mode == ViewMode::ScrollHorizontal;

    // Transform-scaled modes: replace the transform with real geometry,
    // against the still-suspended virtualizers, and restore the focus on the
    // new layout.
    if !is_horizontal {
        engine.commit_geometry(state, t.to, &t.focus);
    }
    // Every scale agrees at the target: the rasters that mount from here are
    // crisp at exactly the size the hosts are already showing, and the stage
    // transform drops out in the same flush that installs the real layout.
    state.viewer.zoom.committed.set(t.to);
    state.viewer.zoom.display.set(t.to);
    state.viewer.zoom.presentation.set(1.0);
    // Releasing the transition last is what un-freezes page/scroll sync and
    // geometry feedback — everything downstream re-runs against a settled
    // scale, never a half-landed one.
    state.viewer.zoom.transition.set(None);
    // Nothing renders inside a transaction; sweep the rasters now that the
    // render scale has moved.
    pdf_engine::api::sweep();
    // The zombies that shielded this transaction keep their raised grace
    // until it expires; only the GRACE resets here (their expiry was fixed
    // when they were evicted). Guarded so a new transaction that opened in
    // the meantime is not stepped on — its own finish will reschedule.
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
