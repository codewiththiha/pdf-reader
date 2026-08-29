//! [`ZoomController`]: the single authority for what the effective zoom is
//! and how every change of it runs.
//!
//! All zoom traffic arrives as commands on `viewer.zoom.commands` — toolbar
//! buttons, keyboard steps, the fit watcher, the follow watcher — and is
//! executed by exactly one effect here. The controller:
//!
//! 1. resolves the command to a target (fit and constraint maths live in
//!    super::target),
//! 2. opens a transition from the scale on screen right now to that target,
//! 3. tweens the live display scale (super::animation), relaying the layout
//!    out through the engine on every frame so the document resizes
//!    continuously under the reader's eyes,
//! 4. and on landing brings the render scale onto the target and releases
//!    the freezes (render suspension, page/scroll synchronisation, geometry
//!    feedback, scroll echo).
//!
//! A container follow (`ZoomCommand::Follow`, one post per frame of a sidebar
//! slide or a window drag) is the one command that does not commit when it
//! lands. Its layout must move every frame or the page squishes, but a raster
//! pass per frame would be a storm, so the transition stays open — holding the
//! same freezes a gesture holds — and a deadline `FOLLOW_SETTLE_MS` after the
//! burst goes quiet commits it once. Every post moves the deadline, so what the
//! reader sees is a page that rides the slide and sharpens when it stops.
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

use crate::state::reader::{ReaderState, ZoomCommand, ZoomTransition};
use crate::viewer::engine::ViewerEngine;

use super::animation::{land, Tween};
use super::config;
use super::target;

/// Does this command hold its crisp commit until the container settles?
///
/// Only a container follow does. Everything else is a single, deliberate change
/// of scale and commits as soon as it lands — deferring it would leave the page
/// stretched for no reason.
pub(crate) fn holds_commit(cmd: ZoomCommand) -> bool {
    matches!(cmd, ZoomCommand::Follow)
}

/// What a container-driven watcher is allowed to do, given the transaction that
/// is open. The watchers ask this instead of reading the transition themselves,
/// so "who owns the scale right now" stays a pipeline question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Nothing is open: post the command as its own transaction, which lands and
    /// commits on the same frame.
    Now,
    /// A follow already holds the burst: retarget it. Its commit is still the
    /// settle deadline's, so a change that arrives mid-burst (a page turn under a
    /// fit, say) rides the same single render instead of forcing its own.
    Follow,
    /// A tweened gesture owns the transaction. Post nothing: resolving container
    /// maths into it and writing the display scale would land the scale BEFORE
    /// the tween had anything to interpolate from — the one-frame snap that made
    /// the zoom control look broken.
    StandDown,
}

/// The gate for a transaction that may be open.
///
/// Keying this off "is a zoom in flight" alone would be just as wrong as ignoring
/// it: the first frame of a slide opens the transaction, so every later frame
/// would bail out and the smooth slide would become a jump at the end.
pub(crate) fn posting_gate(open: Option<ZoomTransition>) -> Gate {
    match open {
        None => Gate::Now,
        Some(t) if t.following => Gate::Follow,
        Some(_) => Gate::StandDown,
    }
}

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

        // The held commit's deadline. A generation counter rather than a
        // stored timer handle: each frame of a burst stamps a new generation
        // and the superseded fires become no-ops, so a post that lands while
        // an earlier deadline is outstanding can never strand the transaction
        // uncommitted (which would strand the freezes with it — nothing would
        // render until the next zoom). Same token idiom `post` uses.
        let settle_gen = StoredValue::new_local(0u64);
        let arm_settle = {
            let settle_engine = engine.clone();
            move || {
                let generation = settle_gen.get_value() + 1;
                settle_gen.set_value(generation);
                let held_engine = settle_engine.clone();
                let _ = set_timeout_with_handle(
                    move || {
                        // Superseded? A later frame of the burst re-armed the
                        // deadline, or a real gesture took the transaction over
                        // and commits itself. Either way this fire owes
                        // nothing.
                        if settle_gen.get_value() != generation {
                            return;
                        }
                        let zoom = state.viewer.zoom;
                        if let Some(t) = zoom.transition.get_untracked() {
                            // Only ever a follow: a transaction that was opened
                            // or replaced in the meantime carries its own commit.
                            if t.following {
                                finish_transition(&state, &held_engine, &t);
                            }
                        }
                    },
                    Duration::from_millis(config::FOLLOW_SETTLE_MS),
                )
                .ok();
            }
        };

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
            // A follow rides the container, so its own commit is deferred. Move
            // the deadline BEFORE deciding the frame is a no-op: while a burst
            // is running the scale may well be pinned (a page clamped at the
            // minimum, a hand-picked zoom capped at `desired` as the window
            // widens), and a commit that lands on the frame the container is
            // still moving rasterises at a width the reader is already past.
            let following = holds_commit(cmd);
            if following {
                arm_settle();
            }
            let display = zoom.display.get_untracked();
            let in_flight = zoom.transition.get_untracked();
            let settled = in_flight.map(|t| t.to).unwrap_or(display);
            if (target - settled).abs() < config::SETTLED_EPSILON {
                // Already there (or already heading there): nothing to move.
                return;
            }
            // `from` is the visual scale RIGHT NOW, so a retarget continues
            // from wherever the eye currently is instead of teleporting.
            // Nothing about POSITION is captured: the layout relayouts
            // continuously and the engine holds the reader's view still
            // itself, frame by frame.
            let mode = state.viewer.mode.get_untracked();
            let transition = ZoomTransition {
                from: display,
                to: target,
                start_ms: js_sys::Date::now(),
                // There is nothing to ease into when the target is whatever the
                // container now allows: a follow that tweened would chase the
                // window instead of sitting in it.
                animate: animate && !following,
                following,
            };
            // Bridge the relayouts before they happen: raise the strips'
            // zombie grace so pages the moving window evicts keep their DOM
            // past the animation's end.
            let retention = config::profile_for(mode).retention;
            engine.vertical.set_retention_grace(retention.grace_ms);
            engine.horizontal.set_retention_grace(retention.grace_ms);
            // The transition goes up BEFORE anything moves, because the frames
            // it holds are exactly the ones that must not feed a measurement or
            // the browser's scroll echo back into the layout being resized.
            zoom.transition.set(Some(transition));
            if following {
                // A follow lands HERE rather than on the next animation frame.
                // The browser runs `ResizeObserver` callbacks after that frame's
                // rAF callbacks, so a landing handed to the tween loop would be
                // painted one frame after the container shrank — and a page row
                // the flex engine may not resize is by then a few pixels wider
                // than the box it has to fit in, which reads as a scrollbar
                // flickering along the whole length of a drag. Landing in this
                // task puts the new size in the same frame as the new width,
                // which is what a continuous follow has to mean.
                land(&state, &engine, &transition);
            } else {
                tween.arm(state, engine.clone());
            }
        });
    }
}

/// Land a transition: bring the render scale onto the target and release the
/// freezes.
///
/// There is no geometry step left to run. The last tween frame (or the first
/// frame of an untweened landing) already relayed the layout out to exactly
/// the target, so all that is left is for the rasters to catch up with the
/// size the hosts are already showing.
///
/// Every transaction ends here, and only the calls differ: a tween and a
/// discrete refit commit on the frame they land, while a container follow is
/// committed by the settle deadline above — once per burst, at the size the
/// container stopped at. Setting the scales is a no-op write when a follow has
/// been landing all along, so a held commit is quiet even when it moves nothing.
pub(crate) fn finish_transition(state: &ReaderState, engine: &ViewerEngine, t: &ZoomTransition) {
    state.viewer.zoom.committed.set(t.to);
    state.viewer.zoom.display.set(t.to);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn open(following: bool) -> ZoomTransition {
        ZoomTransition {
            from: 1.0,
            to: 1.4,
            start_ms: 0.0,
            animate: !following,
            following,
        }
    }

    #[test]
    fn only_a_follow_holds_its_render() {
        assert!(holds_commit(ZoomCommand::Follow));
        for cmd in [
            ZoomCommand::Set(1.5),
            ZoomCommand::Step(1),
            ZoomCommand::Refit,
            ZoomCommand::Constrain,
        ] {
            assert!(!holds_commit(cmd), "{cmd:?} is one deliberate change");
        }
    }

    #[test]
    fn a_slide_may_retarget_itself_but_never_a_gesture() {
        // Idle: a slide may open its own transaction, and a discrete refit
        // commits as soon as it lands.
        assert_eq!(posting_gate(None), Gate::Now);
        // A follow already in flight is retargeted on every frame of the burst,
        // and anything resolving in the meantime rides its held commit.
        assert_eq!(posting_gate(Some(open(true))), Gate::Follow);
        // A tweened gesture keeps its transaction to itself.
        assert_eq!(posting_gate(Some(open(false))), Gate::StandDown);
    }
}
