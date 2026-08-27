//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)`
//! - mode flip -> scroll: `scroll_to_index(Start, Instant)` when re-entering
//!   continuous mode

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use super::nav_mode_flip::mode_flip;

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

/// Must be called once from the app root (ReaderPage), alongside `fit_effect`.
pub fn navigation_sync(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let nav = NavSyncState::new();
    let mode = state.viewer.mode;

    mode_flip(state, virtualizer.clone(), h_virtualizer.clone());

    {
        let suppress = nav.suppress.clone();
        let page = state.viewer.page;
        let v = virtualizer.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::Continuous {
                return;
            }
            let dominant = v.dominant().get() as u32 + 1;
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
            if mode.get() != ViewMode::Continuous {
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
            if mode.get() != ViewMode::Horizontal {
                return;
            }
            let dominant = v.dominant().get() as u32 + 1;
            if page.get_untracked() == dominant {
                return;
            }
            suppress.set(true);
            page.set(dominant);
        });
    }

    {
        // page changes drive the horizontal strip
        let (suppress, page, v, zooming) = (
            nav.suppress.clone(),
            state.viewer.page,
            h_virtualizer.clone(),
            state.viewer.zoom_animating,
        );
        Effect::new(move |_| {
            if mode.get() != ViewMode::Horizontal {
                return;
            }
            // While zooming, the strip is center-anchored (see zoom.rs);
            // re-centering a page here would fight the pinch.
            if zooming.get() {
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
