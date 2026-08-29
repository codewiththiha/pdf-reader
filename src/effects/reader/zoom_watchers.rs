//! The zoom sources: thin reactive watchers that re-ask the zoom controller
//! when the world under the reader changes.
//!
//! Neither watcher computes a scale, writes a zoom signal, or calls the
//! engine — they POST COMMANDS ([`ZoomCommand`]) and the controller's one
//! transition pipeline does the rest. The one question either asks back is
//! `posting_gate`: whether a transaction is already open that it must not
//! disturb, and whether the burst it is holding should carry this change too.
//! That is a gate on posting, not a share of the maths. It is the whole point of
//! the split: fit width, fit page, a sidebar slide, a window resize and a manual
//! `+` all land through the same resolve → relayout → commit path, so they can no
//! longer race along separate code paths.
//!
//! What the two watchers differ in is CADENCE, which is the whole difference
//! between a page that rides a resize and a page that waits for one:
//!
//! - `follow_watcher`: the SPACE the page has moved — a sidebar slide or a
//!   window drag, both of which report a burst of container sizes. It fires on
//!   every frame of that burst, because a scale frozen through it leaves the
//!   page host wider than the box it now has to fit in, and the flex engine
//!   squishes the paper (a letter page went from a 0.77 aspect to 0.58) until
//!   the refit finally lands. Only the crisp RENDER waits: a follow holds its
//!   commit until the container has been quiet, so a slide costs one raster
//!   pass instead of one per frame.
//! - `fit_watcher`: the world under the scale changed in a way that is not a
//!   burst — a view-mode flip, a page turn in a mixed-size book, a new
//!   document. Single events, so they debounce and land through the ordinary
//!   transition, and a page-only change must NOT follow the layout per frame:
//!   scrolling through a book of alternating sizes would re-fit at every row
//!   boundary. It posts the fit when a fit owns the scale and the
//!   shrink-to-fit ceiling when the reader zoomed by hand, so neither case is
//!   left stale against the page under the eyes. Choosing or dropping a fit is
//!   the one event it does not postpone: that is a click, and a click answers
//!   in the frame it lands.
//!
//! Both fires are deliberately UNTWEENED. A refit that tracks a live resize has
//! to land in the frame it was asked for; queueing a 120ms animation against
//! each new width had the page visibly chasing the window. Manual zooms keep
//! their tween — they are one gesture, not a stream.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::math::FitMode;

use crate::components::primitives::hooks::use_timeout::use_debounce;
use crate::state::reader::ZoomCommand;
use crate::state::{ReaderState, SidebarMode};
use crate::viewer::zoom::config::FOLLOW_SETTLE_MS;
use crate::viewer::zoom::coordinator::{Gate, posting_gate};

/// Trailing debounce for a discrete refit: the same window of quiet a held
/// follow waits for before it commits, so a page turn and the end of a resize
/// are settled by one number rather than two that can drift apart.
const REFIT_DEBOUNCE: Duration = Duration::from_millis(FOLLOW_SETTLE_MS);

/// Re-resolve the scale when the world under it changes in a way that is not
/// the container moving: a fit mode chosen or dropped, a view-mode flip, or a
/// page turn. Must be called once from the reader shell (ReaderPage), alongside
/// `follow_watcher`.
///
/// The page matters even to a hand-picked zoom, because the ceiling a manual
/// scale is held to is the fit width OF THE PAGE UNDER THE EYES: a landscape
/// plate at 200 percent shrinks to stay readable instead of sitting cropped,
/// and because the ceiling never writes `desired`, the next portrait page grows
/// back to exactly the zoom the reader chose.
pub fn fit_watcher(state: ReaderState) {
    // Built in the owner that calls us, not inside the effect: the debouncer
    // is one per watcher and disarms itself on cleanup, so a fire cannot
    // land on a reader that has already been disposed.
    let refit = use_debounce(REFIT_DEBOUNCE, move || {
        // Asked again AT THE FIRE, not only when the burst armed it: a gesture
        // that started while the fire was pending owns the transaction, and
        // resolving a fit into it mid-tween is the snap this gate exists for.
        // (A dropped fire is cheap: a manual zoom clears the fit mode, so the
        // reader has taken the scale over anyway, and the next page or
        // container change re-arms this.)
        let cmd = match posting_gate(state.viewer.zoom.transition.get_untracked()) {
            Gate::StandDown => return,
            // A slide is mid-flight: hand the change to its held commit rather
            // than land a second transaction that renders in the middle of the
            // burst. A follow resolves the same question — the fit when a fit
            // owns the scale, the ceiling otherwise — so nothing is lost.
            Gate::Follow => ZoomCommand::Follow,
            Gate::Now if state.viewer.fit.get_untracked() == FitMode::None => {
                ZoomCommand::Constrain
            }
            Gate::Now => ZoomCommand::Refit,
        };
        // Untweened: a fit tracks the window, so it must land in the frame it
        // was resolved in rather than chase it for the length of a tween.
        state.viewer.zoom.post(cmd, false);
    });

    // The fit the last run answered, so "a fit mode was CHOSEN" can be told
    // apart from "the page under a fit changed". Opening a document lands in
    // the chosen branch too, which is right: the first fit is a decision, not a
    // scroll artefact.
    let last_fit = StoredValue::new_local(state.viewer.fit.get_untracked());

    Effect::new(move |_| {
        // Every dependency is a tracked read; none of the values are needed
        // locally — the controller re-reads the world when it resolves. The
        // container is deliberately NOT one of them: a slide or a drag is a
        // stream, and streams are `follow_watcher`'s business.
        let fit = state.viewer.fit.get();
        let chosen = last_fit.get_value() != fit;
        last_fit.set_value(fit);
        let _ = state.viewer.mode.get();
        let _ = state.viewer.page.get();
        if matches!(
            posting_gate(state.viewer.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a zoom is mid-flight; let it settle first
        }
        // A fit the reader just picked is an answer to a click, not a burst: it
        // moves the page NOW and sharpens on the follow's held commit, exactly
        // like riding a resize. Debouncing it would hold the page still for the
        // whole window for no reason.
        if chosen {
            state.viewer.zoom.post(ZoomCommand::Follow, false);
            return;
        }
        // Postpones any pending fire and schedules one at the end of the
        // current burst of changes.
        refit.trigger();
    });
}

/// Follow the space the page has, frame by frame. This is what makes the
/// sidebar slide and the window drag MOVE the canvas instead of snapping it
/// when the drag ends, and it covers both readers: while a fit mode is active
/// the follow re-fits, and while the reader has zoomed by hand it carries the
/// chosen zoom across the change (`min(desired, fit-width)`), so the page
/// shrinks out of the rail's way and grows back to exactly where it was — loss
/// both ways, because the ceiling is computed from the remembered `desired` and
/// never from the live scale times a container ratio.
///
/// Must be called once from the reader shell (ReaderPage), alongside
/// `fit_watcher`.
pub fn follow_watcher(state: ReaderState, sidebar: RwSignal<SidebarMode>) {
    Effect::new(move |_| {
        let _ = state.viewer.container_size.get();
        let _ = state.viewer.page_margin.get();
        // Tracked so a toggle starts the follow on the frame the rail MOVES,
        // not only once its animation has begun resizing the container. The
        // value itself is not: the page is sized from the space that is
        // actually available, whatever took it.
        let _ = sidebar.get();
        // The transaction is read UNTRACKED, on purpose. Tracking it is what
        // forced a one-shot "I just committed, do not re-apply the ceiling"
        // echo onto the old implementation, and this pipeline does not need
        // the echo: after a gesture lands, the ceiling answers the next real
        // container change, which is the case it was written for. Zooming past
        // the fit stays put until then, exactly as it does in every desktop
        // reader.
        if matches!(
            posting_gate(state.viewer.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a gesture owns the transaction; let it land first
        }
        state.viewer.zoom.post(ZoomCommand::Follow, false);
    });
}
