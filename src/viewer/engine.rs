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

use crate::state::reader::ReaderState;

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

    /// Rescale the strip item sizes by `factor` — the ratio between the new
    /// and the current layout scale — anchoring the scroll so the content
    /// under the viewport does not jump.
    ///
    /// This is the single relayout path for the whole viewer, and it runs on
    /// EVERY frame of a zoom: the tween hands it the ratio between the scale
    /// it is about to show and the scale the layout currently has, and the
    /// layout follows continuously. That continuity is the point. The
    /// virtualizer's rescale anchor holds the reader's view steady while the
    /// sizes underneath it move, so there is nothing to capture before the
    /// gesture and nothing to restore after it.
    pub fn relayout_to(&self, state: &ReaderState, factor: f64) {
        if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return; // already at this geometry; nothing to move
        }

        // Vertical strip: heights are the measured CSS column, carried
        // forward by the frame's factor.
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
            self.vertical.rescale(factor, {
                let heights = heights.clone();
                move |index| heights.get(index).copied().unwrap_or(0.0) + gap
            });
        }

        // Horizontal strip: widths are exact (intrinsic × scale + margin),
        // so they are rebuilt from the intrinsic sizes at the scale this
        // relayout lands on rather than from a scaled copy of the previous
        // width — that keeps the running product free of drift across the
        // many small factors a single tween applies.
        let margin = state.viewer.page_margin.get_untracked();
        let widths = state.document.metrics.intrinsic.with_untracked(|sizes| {
            sizes.iter().map(|s| s.width).collect::<Vec<f64>>()
        });
        let new_scale = state.viewer.zoom.display.get_untracked() * factor;
        if !widths.is_empty() {
            self.horizontal.rescale(factor, move |index| {
                widths.get(index).copied().unwrap_or(0.0) * new_scale + 2.0 * margin
            });
        }

        // Keep the reader's own scroll signal in step with the anchored
        // offset the rescale produced.
        let scroll_top = self.vertical.scroll_offset().get_untracked();
        if (scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
            state.viewer.scroll_top.set(scroll_top);
        }

        // Growing content: re-assert the anchored offset one frame later.
        // `rescale` writes the new offset synchronously, but the spacer that
        // gives the scroller its scroll extent is patched by Leptos only
        // after that call returns, so the browser clamps the write against
        // the still-short old extent. The clamp error is worst at the end of
        // a growing document, which is why the jump used to show on the last
        // pages and only while content was growing.
        if factor > 1.0 {
            let v = self.vertical.clone();
            let target_scroll = scroll_top;
            request_animation_frame(move || {
                v.scroll_to_offset(target_scroll, ScrollMode::Instant);
            });
            let hv = self.horizontal.clone();
            let h_scroll = self.horizontal.scroll_offset().get_untracked();
            request_animation_frame(move || {
                hv.scroll_to_offset(h_scroll, ScrollMode::Instant);
            });
        }
    }
}
