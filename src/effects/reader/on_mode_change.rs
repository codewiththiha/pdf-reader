//! Entering a scrolling mode: align the virtualized scroll position to the
//! current page.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

pub(super) fn on_mode_change(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    let mut was_horizontal = state.viewer.mode.get_untracked() == ViewMode::ScrollHorizontal;
    Effect::new(move |_| {
        let continuous = state.viewer.mode.get() == ViewMode::ScrollVertical;
        let horizontal = state.viewer.mode.get() == ViewMode::ScrollHorizontal;
        if continuous && !was_continuous {
            // A fit debounce interrupted by the flip can strand `zoom_animating`
            // true, which would keep every re-mounted page's render effect in
            // the suspended branch — a blank viewer. The flip is an explicit
            // user action, so the flag is released before the re-entry render.
            state.viewer.zoom_animating.set(false);
            let page = state.viewer.page.get_untracked();
            let v = virtualizer.clone();
            request_animation_frame(move || {
                // Fresh container: recalibrate the viewport before anchoring,
                // otherwise the window is computed against stale geometry.
                v.remeasure_container();
                if page > 0 {
                    let index = (page - 1) as usize;
                    v.scroll_to_index(index, Align::Start, ScrollMode::Instant);
                }
            });
        }
        if horizontal && !was_horizontal {
            state.viewer.zoom_animating.set(false);
            let page = state.viewer.page.get_untracked();
            let v = h_virtualizer.clone();
            request_animation_frame(move || {
                v.remeasure_container();
                if page > 0 {
                    let index = (page - 1) as usize;
                    v.scroll_to_index(index, Align::Center, ScrollMode::Instant);
                }
                // Measure a SECOND time, one frame later. On the very first
                // switch the strip's flex row has not always committed its
                // layout by this frame, so `remeasure_container` reads a
                // clientWidth of 0: the core gets a zero viewport, the
                // window resolves to nothing, and no page mounts. The
                // ResizeObserver cannot rescue it either — it was attached
                // before the element had a layout box, so its first entry
                // reports 0 as well. Going out of the mode and back in
                // remounts after layout has settled, which is exactly why
                // that worked around it. One more frame guarantees real
                // geometry and the window mounts its pages.
                let v2 = v.clone();
                request_animation_frame(move || {
                    v2.remeasure_container();
                });
            });
        }
        was_continuous = continuous;
        was_horizontal = horizontal;
        // A mode flip leaves the outgoing view's rasters behind and nothing
        // necessarily renders right after, so the engine's own sweep (which
        // only runs inside a render) would never fire. Release now.
        pdf_engine::api::sweep();
    });
}
