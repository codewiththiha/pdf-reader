//! The continuous-scroll axis: dominant item → page counter, and page counter
//! → scroll position. Each direction is one effect; neither reads the other's
//! output, and the shared `suppress` flag is what keeps the pair from echoing.

use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, Virtualizer};

use crate::state::ReaderState;

use super::gate::JumpGate;
use super::{NavSyncState, page_from_dominant, scroll_mode};

/// The continuous strip's dominant item drives the page counter.
pub(super) fn dominant_to_page(
    state: ReaderState,
    virtualizer: &Virtualizer,
    nav: &NavSyncState,
    mode: RwSignal<ViewMode>,
    zooming: Signal<bool>,
    gate: &Rc<JumpGate>,
) {
    let suppress = nav.suppress.clone();
    let gate = gate.clone();
    let defer = nav.defer;
    let page = state.viewer.page;
    let v = virtualizer.clone();
    Effect::new(move |_| {
        if mode.get() != ViewMode::ScrollVertical {
            return;
        }
        // While on_mode_change is re-anchoring the strip after a mode flip,
        // the strip is still sitting at its initial (unrestored) offset and
        // its dominant would misreport the page — reading it now resets the
        // reader back to (a transient) page 0/1. Stand down; the restored
        // scroll re-runs this effect with the true dominant.
        if defer.get() {
            return;
        }
        let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
        // During a zoom transaction the virtualizer's window is frozen,
        // but a mid-zoom wheel can still rewindow and move the dominant
        // item through no fault of the reader. The zoom anchor already
        // knows the page; syncing it here is what made the page number
        // flicker to a neighbour mid-gesture.
        //
        // TRACKED, because a container follow holds a transaction open for
        // the whole burst of a sidebar slide or a window drag: shrinking the
        // page fits more of the book on screen, so the dominant item
        // legitimately moves while the flag is up, and an untracked guard
        // would drop that page and leave the counter on the old one until
        // the reader scrolled again. Landing on the commit is the same one
        // write, at the moment the scale is final.
        if zooming.get() {
            return;
        }
        // A held navigation replays in this same flush and the strip has
        // not moved yet — correcting the page from the stale dominant now
        // is exactly how the resume jump used to die. Let the replay land
        // first; the re-run it causes reads the TRUE dominant.
        if gate.pending().is_some() {
            return;
        }
        if page.get_untracked() == dominant {
            return;
        }
        suppress.set(true);
        page.set(dominant);
    });
}

/// A page write scrolls the continuous column to that page.
pub(super) fn page_to_scroll(
    state: ReaderState,
    virtualizer: &Virtualizer,
    nav: &NavSyncState,
    mode: RwSignal<ViewMode>,
    zooming: Signal<bool>,
    gate: &Rc<JumpGate>,
) {
    let suppress = nav.suppress.clone();
    let gate = gate.clone();
    let page = state.viewer.page;
    let v = virtualizer.clone();
    Effect::new(move |_| {
        if mode.get() != ViewMode::ScrollVertical {
            return;
        }
        // Scroll restoration is the transaction's job while one is open;
        // letting a page write fight the anchor mid-zoom is the other half
        // of the loop. The gate holds the write instead of losing it, and
        // the tracked `zooming` read is what brings this effect back on
        // the frame the transaction closes, to replay it.
        let page_now = page.get();
        let zooming = zooming.get();
        let Some((target, reassert)) = gate.admit(page_now, zooming) else {
            // A stand-down consumed the run. The echo flag's scroll event
            // is never coming (both arms and the DOM echo stand down for
            // the transaction's duration), so it must not survive to eat
            // the replay either.
            if zooming {
                suppress.set(false);
            }
            return;
        };
        // A replay against a clobbered page signal re-asserts the page:
        // the counter must lead the strip, or the next scroll event would
        // "correct" the strip right back off the jumped-to page.
        if reassert {
            suppress.set(false);
            page.set(target);
        }
        if suppress.get() {
            suppress.set(false);
            return;
        }
        if target == 0 {
            return;
        }
        // The glide is the animation; the jump is not. With the reader's
        // scroll switch off the column lands on the page in one write
        // (`Auto` is what resolves to a smooth scroll when the distance is
        // short, so this is the only decision to make here).
        v.scroll_to_index(
            (target - 1) as usize,
            Align::Start,
            scroll_mode(state),
        );
    });
}
