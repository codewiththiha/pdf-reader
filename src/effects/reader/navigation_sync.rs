//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)`
//! - mode flip -> scroll: `scroll_to_index(Start, Instant)` when re-entering
//!   continuous mode
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use super::on_mode_change::on_mode_change;

/// Shared suppression flag for the two one-way syncs.
pub(super) struct NavSyncState {
    pub suppress: Rc<Cell<bool>>,
}

impl NavSyncState {
    fn new() -> Self {
        Self {
            suppress: Rc::new(Cell::new(false)),
        }
    }
}

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn navigation_sync(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let nav = NavSyncState::new();
    let mode = state.viewer.mode;
    // The tracked form of "a zoom transaction is in flight", for the effects
    // that must RESYNC when one lands (below); the untracked
    // `zooming_now()` stays on the ones that must only not fight it.
    let zooming = state.viewer.zooming();

    on_mode_change(state, virtualizer.clone(), h_virtualizer.clone());

    {
        let suppress = nav.suppress.clone();
        let page = state.viewer.page;
        let v = virtualizer.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollVertical {
                return;
            }
            let dominant = v.dominant().get() as u32 + 1;
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
            if page.get_untracked() == dominant {
                return;
            }
            suppress.set(true);
            page.set(dominant);
        });
    }

    {
        let suppress = nav.suppress.clone();
        let page = state.viewer.page;
        let v = virtualizer.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollVertical {
                return;
            }
            // Scroll restoration is the transaction's job; letting a page
            // write fight the anchor mid-zoom is the other half of the loop.
            //
            // Untracked on purpose, unlike the dominant-page arm above: this
            // one must not run just because a transaction CLOSED, or every zoom
            // commit would scroll the top of the current page back under the
            // reader's eyes.
            if state.viewer.zooming_now() {
                return;
            }
            let page = page.get();
            if suppress.get() {
                suppress.set(false);
                return;
            }
            if page == 0 {
                return;
            }
            v.scroll_to_index((page - 1) as usize, Align::Start, ScrollMode::Auto);
        });
    }

    {
        // dominant page follows the horizontal strip
        let (suppress, page, v) = (nav.suppress.clone(), state.viewer.page, h_virtualizer.clone());
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollHorizontal {
                return;
            }
            let dominant = v.dominant().get() as u32 + 1;
            // Tracked for the same reason as the vertical arm: a follow keeps
            // the window frozen for a whole burst, and this is what catches the
            // page up when it lands.
            if zooming.get() {
                return;
            }
            if page.get_untracked() == dominant {
                return;
            }
            suppress.set(true);
            page.set(dominant);
        });
    }

    {
        // page changes drive the horizontal strip
        let (suppress, page, v) = (nav.suppress.clone(), state.viewer.page, h_virtualizer.clone());
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollHorizontal {
                return;
            }
            // While a zoom transaction is in flight the anchor owns the
            // strip position; re-centering a page here would fight it.
            if state.viewer.zooming_now() {
                return;
            }
            let page = page.get();
            if suppress.get() {
                suppress.set(false);
                return;
            }
            if page == 0 {
                return;
            }
            v.scroll_to_index((page - 1) as usize, Align::Center, ScrollMode::Auto);
        });
    }
}
