//! [`ViewerEngine`]: the single owner of the virtualized scroll geometry.
//!
//! The core contract is that the engine is the *only* place layout is
//! rescaled. Zoom and fit compute a *target* and ask the engine to apply a
//! scale factor; the engine owns the relayout so a gesture and a refit
//! cannot diverge along separate code paths. Non-rescale geometry reads
//! (dominant page, scroll-to-page) still go through the virtualizers
//! directly, but only in the per-mode navigation code.
//!
//! There are TWO ways in, and the view mode picks between them:
//!
//! - [`ViewerEngine::relayout_to`] moves the layout continuously, once per
//!   animation frame. The horizontal strip uses this: its items are laid out
//!   side by side in a flex row, so scaling them through a CSS transform
//!   fights the browser's own flow, and letting the virtualizer's rescale
//!   anchor do the work is both smoother and cheaper.
//! - [`ViewerEngine::commit_geometry`] moves the layout exactly once, when a
//!   transition lands. The vertical strip and the paginated modes use this:
//!   their content surface is scaled by a CSS transform for the duration of
//!   the tween, so the geometry underneath stays untouched until the commit
//!   replaces the transform with real layout.

use leptos::prelude::*;
use pdf_core::layout::{ViewMode, TOOLBAR_H};
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::primitives::hooks::dom::page_list;
use crate::state::reader::{ReaderState, ZoomFocus};
use crate::viewer::zoom::anchor;

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
    /// This is the HORIZONTAL strip's zoom path, and it runs on EVERY frame
    /// of a zoom: the tween hands it the ratio between the scale it is about
    /// to show and the scale the layout currently has, and the layout
    /// follows continuously. That continuity is the point. The virtualizer's
    /// rescale anchor holds the reader's view steady while the sizes
    /// underneath it move, so there is nothing to capture before the gesture
    /// and nothing to restore after it.
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

    /// Commit one zoom transaction's geometry in a single step: move every
    /// strip's item sizes from the committed scale to `target`, then put the
    /// focus back on the new layout.
    ///
    /// This is the VERTICAL strip's (and the paginated modes') zoom path. It
    /// runs once, when a transition lands — deliberately NOT per frame. For
    /// the whole tween those modes scale their content surface through one
    /// CSS transform, so the virtualizers keep the old geometry (the mounted
    /// window cannot churn, the dominant item cannot move) and this single
    /// commit moves geometry, rasters and scroll together, replacing the
    /// transform with real layout at exactly the same visual size.
    ///
    /// Scroll restoration is pixel arithmetic off the page centre: the focus
    /// names the page and the viewport pixels its centre must land on, and
    /// the new offsets are computed against the NEW geometry —
    /// mathematically, because the DOM has not re-laid out yet at this point
    /// (its `scrollWidth`/`scrollHeight` still answer the pre-scale extent).
    ///
    /// The main-axis restore is SYNCHRONOUS: the virtualizer's layout and
    /// signals are already updated in-tick by `rescale`, so commanding the
    /// offset immediately closes what would be a one-frame gap where the
    /// surface had committed but the scroll still sat at the old position.
    /// The cross axis (the DOM scroller's own `scrollLeft`) is written
    /// directly and re-asserted once on the next frame, after the spacer has
    /// laid out at the new extent.
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
        // exact screen pixels it was captured at — synchronously on the main
        // axis, directly on the cross axis with one next-frame re-assert.
        //
        // Only the vertical strip needs it. The horizontal strip never gets
        // here (it relayouts per frame), and the paginated layouts have no
        // strip scroll at all — the page itself is the position and the
        // layout remounts on `viewer.page`.
        let count = state.document.num_pages.get_untracked() as usize;
        let index = focus.page.saturating_sub(1) as usize;
        if state.viewer.mode.get_untracked() == ViewMode::ScrollVertical {
            let (new_origin_x, new_origin_y) = anchor::page_center_origin(
                self,
                state,
                ViewMode::ScrollVertical,
                index,
                count,
                target,
            );
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
    }
}
