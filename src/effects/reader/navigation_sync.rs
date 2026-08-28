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
            if state.viewer.zooming_now() {
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
            if state.viewer.zooming_now() {
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
