//! [`ViewerEngine`]: the single owner of the virtualized scroll geometry.
//!
//! The core contract is that the engine is the *only* place layout is
//! rescaled. Zoom and fit compute a *target* and ask the engine to apply a
//! scale factor; the engine owns the relayout (both the vertical and the
//! horizontal path) so a gesture and a refit cannot diverge along separate
//! code paths. Non-rescale geometry reads (dominant page, scroll-to-page)
//! still go through the virtualizers directly, but only in the per-mode
//! navigation code.

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::ReaderState;

/// Wraps the reader's two strip virtualizers and centralises the viewer's one
/// relayout path. The vertical (continuous) and horizontal strips stay as
/// separate virtualizers — they are created as separate hooks in
/// `ReaderPage` — but resizing a strip's items is done only here.
#[derive(Clone)]
pub struct ViewerEngine {
    /// The continuous (vertical) strip's virtualizer.
    pub vertical: Virtualizer,
    /// The horizontal strip's virtualizer.
    pub horizontal: Virtualizer,
}

impl ViewerEngine {
    pub fn new(vertical: Virtualizer, horizontal: Virtualizer) -> Self {
        Self { vertical, horizontal }
    }

    /// Rescale the strip item sizes by `factor` (the ratio between the new and
    /// current layout scale), anchoring the scroll so the content under the
    /// viewport does not jump.
    ///
    /// This is the single relayout path for the whole viewer: it keeps the
    /// vertical `css_heights` + vertical strip and the horizontal strip in
    /// step, exactly as the old `relayout_to` did, so a gesture and a refit
    /// cannot diverge along separate code paths.
    pub fn relayout_scale(&self, state: &ReaderState, factor: f64) {
        if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return;
        }

        state.document.metrics.css_heights.update(|heights| {
            for height in heights.iter_mut() {
                *height *= factor;
            }
        });

        let heights = state
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.clone());
        if !heights.is_empty() {
            let gap = state.viewer.page_gap.get_untracked();
            self.vertical.rescale(
                factor,
                {
                    let heights = heights.clone();
                    move |index| heights.get(index).copied().unwrap_or(0.0) + gap
                },
            );
            relayout_vertical_scroll(state, &self.vertical, factor);
        }

        // Horizontal strip: widths are exact (intrinsic × scale + margin).
        relayout_horizontal(state, &self.horizontal, factor);
    }
}

/// Re-assert the anchored scroll one frame later. `rescale` clamps the new
/// offset against the new layout and writes it synchronously, but the spacer
/// that gives the scroller its `scrollHeight` is patched by Leptos only after
/// that call returns, so the browser clamps the write to the still-short old
/// height. The clamp error is worst at the end of a growing document, which is
/// why the jump showed on the last pages (and only when content was growing —
/// a sidebar CLOSING). Re-asserting on the next frame, after the spacer has
/// laid out, removes it.
fn relayout_vertical_scroll(state: &ReaderState, v: &Virtualizer, factor: f64) {
    let scroll_top = v.scroll_offset().get_untracked();
    if (scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
        state.viewer.scroll_top.set(scroll_top);
    }
    // Only re-assert on zoom-in. Zooming out shrinks the content, so the
    // browser keeps the offset within the (now longer) scroll range on its
    // own and there is nothing clamped to recover. Zooming in grows content
    // past the old spacer height, so the clamped write needs a re-assert.
    if factor > 1.0 {
        let v = v.clone();
        let target_scroll = scroll_top;
        request_animation_frame(move || {
            v.scroll_to_offset(target_scroll, ScrollMode::Instant);
        });
    }
}

/// The horizontal strip's widths are exact (intrinsic × scale + margin), so
/// the rescale is pure width math; the vertical centring is a single
/// multiplication once the anchored rescale has landed. (`scrollLeft` is the
/// core's; `scrollTop` is the DOM scroller's alone.)
fn relayout_horizontal(state: &ReaderState, hv: &Virtualizer, factor: f64) {
    let new_scale = state.viewer.zoom.layout.get_untracked() * factor;
    let margin = state.viewer.page_margin.get_untracked();
    let widths = state.document.metrics.intrinsic.with_untracked(|sizes| {
        sizes.iter().map(|s| s.width).collect::<Vec<f64>>()
    });
    if widths.is_empty() {
        return;
    }
    let list = crate::components::primitives::hooks::dom::h_page_list();
    let (vh, old_top) = match &list {
        Some(el) => (el.client_height() as f64, el.scroll_top() as f64),
        None => (0.0, 0.0),
    };
    hv.rescale(factor, move |index| {
        widths.get(index).copied().unwrap_or(0.0) * new_scale + 2.0 * margin
    });
    if let Some(el) = list {
        if vh > 1.0 {
            let tallest = state.document.metrics.intrinsic.with_untracked(|sizes| {
                sizes.iter().map(|s| s.height).fold(0.0, f64::max)
            });
            let new_total = vh.max(tallest * new_scale);
            if new_total > vh + 1.0 {
                let center = old_top + vh / 2.0;
                let target = (center * factor - vh / 2.0).clamp(0.0, new_total - vh);
                el.set_scroll_top(target as i32);
            } else if old_top > 0.0 {
                el.set_scroll_top(0);
            }
        }
    }
}
