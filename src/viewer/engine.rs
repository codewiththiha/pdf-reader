//! [`ViewerEngine`]: the single owner of the virtualized scroll geometry.
//!
//! The core contract is that the engine is the *only* place layout is
//! rescaled. Zoom and fit compute a *target* and ask the engine to apply a
//! scale factor; the engine owns the relayout so a gesture and a refit
//! cannot diverge along separate code paths. Non-rescale geometry reads
//! (dominant page, scroll-to-page) still go through the virtualizers
//! directly, but only in the per-mode navigation code.
//!
//! It runs on EVERY frame of a zoom: the tween hands it the ratio between
//! the scale it is about to show and the scale the layout currently has, and
//! the layout follows continuously. Scaling the layout for real is what
//! keeps a zoom stable. The alternative — one CSS transform over a surface
//! whose geometry is frozen — cannot work here, because a transform scales
//! the page gaps along with the pages while the layout deliberately does
//! not (see `virtual_list::anchor::rescale_anchor`). Every gap above the
//! reader accumulates error through the tween and the whole sum lands at
//! once when the transform is swapped for real geometry, which reads as the
//! document jumping.
//!
//! Anchoring is therefore explicit and gap-aware: the engine works out which
//! document point sits under the viewport centre, rescales, finds where that
//! same point lands in the new geometry, and puts it back under the viewport
//! centre. Page interiors scale; the fixed gap between pages does not.

use leptos::prelude::*;
use pdf_core::layout::TOOLBAR_H;
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

    /// Rescale both strips by `factor` — the ratio between the new and the
    /// current layout scale — holding the document point under the viewport
    /// centre exactly where it is.
    pub fn relayout_to(&self, state: &ReaderState, factor: f64) {
        if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return; // already at this geometry; nothing to move
        }

        self.relayout_vertical(state, factor);

        // Horizontal strip: widths are exact (intrinsic × scale + margin),
        // so they are rebuilt from the intrinsic sizes at the scale this
        // relayout lands on rather than from a scaled copy of the previous
        // width — that keeps the running product free of drift across the
        // many small factors a single tween applies. The virtualizer's own
        // rescale anchor holds the cross-axis position.
        let margin = state.viewer.page_margin.get_untracked();
        let widths = state.document.metrics.intrinsic.with_untracked(|sizes| {
            sizes.iter().map(|s| s.width).collect::<Vec<f64>>()
        });
        let new_scale = state.viewer.zoom.display.get_untracked() * factor;
        if !widths.is_empty() {
            self.horizontal.rescale(factor, move |index| {
                widths.get(index).copied().unwrap_or(0.0) * new_scale + 2.0 * margin
            });
            let hv = self.horizontal.clone();
            let h_scroll = self.horizontal.scroll_offset().get_untracked();
            request_animation_frame(move || {
                hv.scroll_to_offset(h_scroll, ScrollMode::Instant);
            });
        }
    }

    /// Rescale the vertical strip and put the document point that was under
    /// the viewport centre back under the viewport centre.
    ///
    /// The anchor is computed from the measurement store rather than left to
    /// the virtualizer's own rescale anchor because the anchor has to survive
    /// a scale the layout applies unevenly: page heights multiply by the
    /// factor, the gap between pages stays put. Walking the column is the
    /// only way to get that exactly right.
    fn relayout_vertical(&self, state: &ReaderState, factor: f64) {
        let gap = state.viewer.page_gap.get_untracked();
        let (_, vh) = state.viewer.container_size.get_untracked();
        let scroll_top = self.vertical.scroll_offset().get_untracked();

        // The strip's content starts TOOLBAR_H below the scroller's origin —
        // the pages scroll under a fixed toolbar — so the content point under
        // the middle of the window sits that band short of half the height.
        let centre_in_viewport = vh / 2.0;
        let centre_y_doc = (scroll_top + centre_in_viewport - TOOLBAR_H).max(0.0);

        let old_heights = state
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.clone());
        if old_heights.is_empty() {
            return; // nothing measured yet; no layout to hold still
        }

        // 1. Which page is under the centre, and how far into it.
        let mut centre_index = 0usize;
        let mut offset_inside = 0.0f64;
        {
            let mut offset = 0.0;
            let last = old_heights.len() - 1;
            for (i, height) in old_heights.iter().enumerate() {
                let item = *height + gap;
                if offset + item > centre_y_doc || i == last {
                    centre_index = i;
                    offset_inside = centre_y_doc - offset;
                    break;
                }
                offset += item;
            }
        }

        // 2. Scale the shared measurement store, then rebuild the strip's
        //    layout from it.
        state.document.metrics.css_heights.update(|heights| {
            for height in heights.iter_mut() {
                *height *= factor;
            }
        });
        let new_heights = state
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.clone());
        self.vertical.rescale(factor, {
            let new_heights = new_heights.clone();
            move |index| new_heights.get(index).copied().unwrap_or(0.0) + gap
        });

        // 3. Where that same point lands now: the scaled extent of every page
        //    above it, plus its own scaled offset inside the page it is in. A
        //    centre that fell in the gap keeps the unscaled remainder, because
        //    the gap is fixed chrome and never scales.
        let mut new_centre_y_doc = 0.0;
        for height in new_heights.iter().take(centre_index) {
            new_centre_y_doc += *height + gap;
        }
        let old_h = old_heights.get(centre_index).copied().unwrap_or(0.0);
        new_centre_y_doc += if offset_inside <= old_h {
            offset_inside * factor
        } else {
            old_h * factor + (offset_inside - old_h)
        };

        // 4. Scroll so it is back under the middle of the window. The ceiling
        //    is the virtualizer's own (`total − viewport`), not the DOM's: it
        //    does not know about the toolbar band, and `scroll_to_offset`
        //    clamps to it, so adopting the larger DOM range here would leave
        //    `viewer.scroll_top` disagreeing with the offset that actually
        //    landed at the very end of a document.
        let max_scroll = (self.vertical.total_size().get_untracked() - vh).max(0.0);
        let new_scroll_top =
            (new_centre_y_doc + TOOLBAR_H - centre_in_viewport).clamp(0.0, max_scroll);

        if (new_scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
            state.viewer.scroll_top.set(new_scroll_top);
        }

        // Synchronous: `rescale` has already updated the virtualizer's layout
        // and signals in this tick, so commanding the offset now lands on the
        // right frame. Deferring it left a one-frame gap where the geometry
        // had moved and the scroll had not.
        self.vertical
            .scroll_to_offset(new_scroll_top, ScrollMode::Instant);

        // Growing content: re-assert one frame later. The spacer that gives
        // the scroller its scroll extent is patched by Leptos only after
        // `rescale` returns, so the browser clamps the write against the
        // still-short old extent — worst at the end of a growing document.
        if factor > 1.0 {
            let v = self.vertical.clone();
            request_animation_frame(move || {
                v.scroll_to_offset(new_scroll_top, ScrollMode::Instant);
            });
        }
    }
}
