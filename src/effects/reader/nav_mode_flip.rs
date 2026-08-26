//! Entering continuous mode: align the virtualized scroll position to the
//! current page.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

pub(super) fn mode_flip(state: ReaderState, virtualizer: Virtualizer) {
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::Continuous;
    Effect::new(move |_| {
        let continuous = state.viewer.mode.get() == ViewMode::Continuous;
        if continuous && !was_continuous {
            let page = state.viewer.page.get_untracked();
            if page > 0 {
                let v = virtualizer.clone();
                let index = (page - 1) as usize;
                request_animation_frame(move || {
                    v.scroll_to_index(index, Align::Start, ScrollMode::Instant);
                });
            }
        }
        was_continuous = continuous;
    });
}
