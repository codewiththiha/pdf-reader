//! The vertical text strip's size model: pages measured in BLOCKS, not A4.
//!
//! The three paged modes (single, spread, horizontal) lay text pages out as
//! real A4 sheets: hosts are `A4 × scale`, the size model is `PAGE_HEIGHT`,
//! and the strip gap is the normal page gap. The vertical strip is the one
//! place the formats diverge from that shape — the requirement is that it
//! reads as one continuous column with NO visible pages.
//!
//! The trick is that "no pages" is a PRESENTATION fact, not a model fact:
//! the virtualizer still virtualizes page-cut units (so the zoom engine's
//! anchored relayout, search reveal, navigation sync and auto-scroll all
//! serve text unchanged), but each unit is sized to the SUM of its blocks'
//! heights and laid out with a zero gap. Every block then carries its own
//! paragraph space — including the last block of a page, whose space falls
//! exactly on the cut — so the rhythm is identical at and between cuts, and
//! the invisible page boundary is also an *unmeasurable* one.
//!
//! This effect projects that model into `css_heights` (the store the zoom
//! engine anchors against and rescales in place every frame) whenever the
//! inputs move, and restores the A4 model the moment the reader leaves the
//! vertical mode or the format stops being text. It never tracks the zoom
//! scale: it writes at the settled scale, and the engine's per-frame
//! factors do the scaling afterwards.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use pdf_core::layout::ViewMode;
use text_core::page::PAGE_HEIGHT;

use crate::state::AppState;

/// The summed block height of one page cut, at scale 1.
fn flow_height(
    cut: &text_core::pager::PageCut,
    heights: &[f64],
) -> f64 {
    heights
        .iter()
        .skip(cut.start)
        .take(cut.count)
        .sum()
}

/// Install the projection. Needs the vertical strip's virtualizer because a
/// model change must rebuild its layout in the same flush.
pub fn text_layout(state: AppState, vertical: Virtualizer) {
    Effect::new(move |_| {
        let format = state.reader.document.format.get();
        let mode = state.reader.viewer.mode.get();
        let cuts = state.reader.text.cuts.get();
        let heights = state.reader.text.heights.get();
        if !format.is_text() {
            return;
        }
        let flow = mode == ViewMode::ScrollVertical;
        let scale = state.reader.viewer.zoom.visual_scale();
        let sizes: Vec<f64> = if flow {
            cuts.iter().map(|cut| flow_height(cut, &heights) * scale).collect()
        } else {
            vec![PAGE_HEIGHT * scale; cuts.len()]
        };

        // Skip the write (and the relayout) when the model already agrees —
        // a zoom tick rescales the store in place, and this effect must not
        // fight the engine back to the settled scale mid-tween.
        let same = state
            .reader
            .document
            .metrics
            .css_heights
            .with_untracked(|store| store.len() == sizes.len() && store.iter().zip(&sizes).all(|(a, b)| (a - b).abs() < 0.5));
        if same {
            return;
        }
        state.reader.document.metrics.css_heights.set(sizes.clone());
        // Gap only separates REAL pages: the flow strip lays its units edge
        // to edge, and the last block's paragraph space provides the rhythm
        // across the invisible cut (see the module docs).
        let gap = if flow { 0.0 } else { state.reader.viewer.page_gap.get_untracked() };
        vertical.rescale(1.0, move |index| sizes.get(index).copied().unwrap_or(0.0) + gap);
    });
}
