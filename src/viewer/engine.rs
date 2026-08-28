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

use crate::components::primitives::hooks::dom::{h_page_list, page_list};
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
    /// Scroll restoration is pixel arithmetic off the page centre: the
    /// focus names the page and the viewport pixels its centre must land
    /// on, and the new offsets are computed against the NEW geometry —
    /// mathematically, because the DOM has not re-laid out yet at this
    /// point (its `scrollWidth`/`scrollHeight` still answer the pre-scale
    /// extent).
    ///
    /// The main-axis restore is SYNCHRONOUS: the virtualizer's layout and
    /// signals are already updated in-tick by `rescale`, so commanding the
    /// offset immediately closes what used to be a one-frame gap where the
    /// surface had committed but the scroll still sat at the old position.
    /// The cross axis (the DOM scroller's own dimension — `scrollLeft` on
    /// the vertical strip, `scrollTop` on the horizontal one) is written
    /// directly and re-asserted once on the next frame, after the spacer
    /// has laid out at the new extent.
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
        // exact screen pixels it was captured at — synchronously on the
        // main axis (the virtualizer already holds the new geometry, and
        // deferring here was the one-frame flicker), directly on the cross
        // axis with a single next-frame re-assert (that axis is the DOM
        // scroller's alone, and its extent only exists after the spacer
        // lays out).
        let count = state.document.num_pages.get_untracked() as usize;
        let index = focus.page.saturating_sub(1) as usize;
        match state.viewer.mode.get_untracked() {
            ViewMode::ScrollVertical => {
                let (new_origin_x, new_origin_y) =
                    anchor::page_center_origin(self, state, ViewMode::ScrollVertical, index, count, target);
                let new_scroll_top = new_origin_y + TOOLBAR_H - focus.viewport_offset_y;
                let new_scroll_left = new_origin_x - focus.viewport_offset_x;

                self.vertical.scroll_to_offset(new_scroll_top, ScrollMode::Instant);

                if let Some(el) = page_list() {
                    el.set_scroll_left(new_scroll_left as i32);
                    request_animation_frame(move || {
                        if let Some(el) = page_list() {
                            el.set_scroll_left(new_scroll_left as i32);
                        }
                    });
                }
            }
            ViewMode::ScrollHorizontal => {
                let (new_origin_x, new_origin_y) =
                    anchor::page_center_origin(self, state, ViewMode::ScrollHorizontal, index, count, target);
                let new_scroll_left = new_origin_x - focus.viewport_offset_x;
                let new_scroll_top = new_origin_y - focus.viewport_offset_y;

                self.horizontal.scroll_to_offset(new_scroll_left, ScrollMode::Instant);

                if let Some(el) = h_page_list() {
                    el.set_scroll_top(new_scroll_top as i32);
                    request_animation_frame(move || {
                        if let Some(el) = h_page_list() {
                            el.set_scroll_top(new_scroll_top as i32);
                        }
                    });
                }
            }
            // Paginated layouts have no strip scroll; the page itself is the
            // position and the layout remounts on `viewer.page`.
            _ => {}
        }
    }
}
