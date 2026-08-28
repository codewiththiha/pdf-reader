//! [`ViewerEngine`]: the single owner of the virtualized scroll geometry.
//!
//! The core contract is that the engine is the *only* place layout is
//! rescaled, and that it happens exactly once per zoom transaction — at the
//! commit boundary, never per animation frame. The zoom controller decides
//! the target and owns the anchor; the engine translates that into the one
//! geometry change: scale the measurement store, rescale both strips, and
//! restore the document anchor on the new layout. Non-rescale geometry
//! reads (dominant page, scroll-to-page) still go through the virtualizers
//! directly, but only in the per-mode navigation code.

use leptos::prelude::*;
use pdf_core::layout::{ViewMode, TOOLBAR_H};
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::primitives::hooks::dom::h_page_list;
use crate::state::reader::{ReaderState, ZoomFocus};
use crate::viewer::zoom::anchor;

/// Wraps the reader's two strip virtualizers and centralises the viewer's
/// one geometry commit. The vertical (continuous) and horizontal strips stay
/// as separate virtualizers — they are created as separate hooks in
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

    /// Commit one zoom transaction's geometry: move every strip's item
    /// sizes from the committed scale to `target`, then put the focus back
    /// on the new layout.
    ///
    /// This runs once, when a transition lands. It is deliberately NOT part
    /// of the animation: during the tween the virtualizers keep the old
    /// geometry (the mounted window cannot churn, the dominant item cannot
    /// move) while the presentation stage scales the whole surface
    /// visually; this single commit moves geometry, rasters and scroll
    /// together.
    ///
    /// Scroll restoration is document-logical, not pixel arithmetic: the
    /// focus names a page and fractions through it, so it stays correct
    /// even though the intermediate scale never had a consistent geometry.
    ///
    /// The physical scroll write is ONE explicit synchronisation step, one
    /// frame later: `rescale`'s own write happens before Leptos has patched
    /// the spacer to the new extent, so the browser clamps it against the
    /// old scroll range. Rescale's centre pin and the focus are the same
    /// point, so the intermediate frame is already visually right; the
    /// single deferred write refines it to the exact fractional offset.
    pub fn commit_geometry(&self, state: &ReaderState, target: f64, focus: &ZoomFocus) {
        let from = state.viewer.zoom.committed.get_untracked();
        let factor = target / from;
        if !factor.is_finite() || factor <= 0.0 || (factor - 1.0).abs() < 1e-12 {
            return; // already at this geometry; nothing to move
        }

        // Vertical strip: heights are the measured CSS column, scaled by the
        // commit factor. (`rescale` rebuilds the layout from the closure —
        // the factor feeds its centre-pinned anchor, which the logical
        // restore below then overrides.)
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

        // Horizontal strip: widths are exact (intrinsic × scale + margin).
        let margin = state.viewer.page_margin.get_untracked();
        let widths = state.document.metrics.intrinsic.with_untracked(|sizes| {
            sizes.iter().map(|s| s.width).collect::<Vec<f64>>()
        });
        if !widths.is_empty() {
            self.horizontal.rescale(factor, move |index| {
                widths.get(index).copied().unwrap_or(0.0) * target + 2.0 * margin
            });
        }

        // With the new layout in place, put the PAGE CENTRE back on the
        // exact screen pixels it was captured at — the page the reader is
        // on stays glued to one spot; everything else scales around it.
        // ONE deferred synchronisation step (see the method docs): the
        // spacer must have laid out at the new extent before the offset can
        // be written without the browser clamping it against the old range.
        let count = state.document.num_pages.get_untracked() as usize;
        let index = focus.page.saturating_sub(1) as usize;
        match state.viewer.mode.get_untracked() {
            ViewMode::ScrollVertical => {
                let (_, new_origin_y) =
                    anchor::page_center_origin(self, ViewMode::ScrollVertical, index, count);
                let new_scroll_top = new_origin_y + TOOLBAR_H - focus.viewport_offset_y;
                let v = self.vertical.clone();
                request_animation_frame(move || {
                    v.scroll_to_offset(new_scroll_top, ScrollMode::Instant);
                });
            }
            ViewMode::ScrollHorizontal => {
                let (new_origin_x, new_origin_y) =
                    anchor::page_center_origin(self, ViewMode::ScrollHorizontal, index, count);
                let new_scroll_left = new_origin_x - focus.viewport_offset_x;
                let new_scroll_top = new_origin_y - focus.viewport_offset_y;
                let hv = self.horizontal.clone();
                request_animation_frame(move || {
                    hv.scroll_to_offset(new_scroll_left, ScrollMode::Instant);
                    // Cross axis: clamped against the (possibly vanished)
                    // overflow band, so a zoom out past the point where the
                    // page fits simply parks at the band's edge instead of
                    // throwing the position away.
                    if let Some(el) = h_page_list() {
                        let max_y = (el.scroll_height() as f64 - el.client_height() as f64).max(0.0);
                        el.set_scroll_top(new_scroll_top.clamp(0.0, max_y) as i32);
                    }
                });
            }
            // Paginated layouts have no strip scroll; the page itself is the
            // position and the layout remounts on `viewer.page`.
            _ => {}
        }
    }
}
