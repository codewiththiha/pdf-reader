//! Blend backdrop driver: publishes the blend scope, the page pair, and the
//! scroll progress between them to the engine.
//!
//! The engine owns the COLOURS — detection off the raw rasters, the
//! per-document cache, the all-pages scan, the per-page palette, the
//! interpolation. The shell owns the GEOMETRY: which page the reader is on,
//! and how far the viewport has travelled toward the next one. This module
//! is the bridge — three effects, one per fact the engine cannot learn on
//! its own.
//!
//! The progress math deliberately uses the virtualizer's own coordinate
//! convention (viewport = `[scroll, scroll + height]` against item offsets
//! whose sizes fold in the trailing gap) — the same convention the dominant
//! tracker uses, so the blend and the page counter always agree on which
//! page is current, down to the same toolbar-band offset the tracker
//! already accepts.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use pdf_core::settings::BlendScope;

use crate::state::AppState;

/// Wire the blend backdrop driver. Called once from ReaderPage, alongside
/// the other reader effects.
pub fn blend_backdrop(state: AppState) {
    let settings = state.settings;
    let viewer = state.reader.viewer;
    let num_pages = state.reader.document.num_pages;
    let heights = state.reader.document.metrics.css_heights;

    // The scope: which pages the engine samples the paper colour from. Sent
    // whether blend mode is on or not — the scope setting outlives the
    // switch, and the engine's sampling is idle either way until a raster
    // renders.
    Effect::new(move |_| {
        let scope = settings.with(|st| st.layout.blend_scope);
        pdf_engine::api::set_blend_scope(scope);
    });

    // The pair: the dominant page and the one after it. Only the vertical
    // strip has a "next" page to blend into — everywhere else (the paged
    // modes, the horizontal strip) the pair is the page itself and the
    // backdrop simply switches with the page turn. The engine stores the
    // pair in every scope so flipping to continuous mid-book has the right
    // pair already in place.
    Effect::new(move |_| {
        let page = viewer.page.get();
        let total = num_pages.get();
        if page == 0 {
            return;
        }
        let next = if viewer.mode.get() == ViewMode::ScrollVertical && page < total {
            page + 1
        } else {
            page
        };
        pdf_engine::api::set_blend_pages(page, next);
    });

    // The progress: the next page's share of the viewport's visible page
    // paint, 0..1. Per scroll tick, and only while the continuous scope is
    // actually driving the backdrop — the other scopes ignore progress, and
    // blend mode off means no backdrop to drive.
    Effect::new(move |_| {
        if !settings.with(|st| st.layout.blend_mode) {
            return;
        }
        if settings.with(|st| st.layout.blend_scope) != BlendScope::Continuous {
            return;
        }
        if viewer.mode.get() != ViewMode::ScrollVertical {
            return;
        }
        let page = viewer.page.get();
        let scroll = viewer.scroll_top.get();
        let (_, viewport_h) = viewer.container_size.get();
        let gap = viewer.page_gap.get();
        let column = heights.get();
        pdf_engine::api::set_blend_progress(blend_mix(&column, gap, scroll, viewport_h, page));
    });
}

/// Scroll progress from page `cur` (1-based) toward the page after it: the
/// next page's share of the viewport's visible page paint. `0` while the
/// next page is out of sight, `1` once it owns everything the current page
/// did — which is exactly when the dominant tracker hands over to it, so
/// the colour is already the next page's the moment it becomes current.
pub(crate) fn blend_mix(heights: &[f64], gap: f64, scroll: f64, viewport: f64, cur: u32) -> f64 {
    // `cur` is the 1-based page the reader is on; the column is 0-based.
    let idx = (cur as usize).saturating_sub(1);
    let Some(&cur_h) = heights.get(idx) else {
        return 0.0; // column not measured yet
    };
    let Some(&next_h) = heights.get(idx + 1) else {
        return 0.0; // the last page has nothing to blend into
    };
    let cur_top = paint_top(heights, gap, idx);
    let next_top = cur_top + cur_h + gap; // the trailing gap is chrome, not paint
    pair_mix(cur_top, cur_h, next_top, next_h, scroll, scroll + viewport)
}

/// The main-axis offset of page `idx`'s paint: every page above contributes
/// its height plus its trailing gap, matching the strip's `height + gap`
/// item sizes.
fn paint_top(heights: &[f64], gap: f64, idx: usize) -> f64 {
    heights.iter().take(idx).map(|h| h + gap).sum()
}

fn pair_mix(top_a: f64, h_a: f64, top_b: f64, h_b: f64, view_top: f64, view_bottom: f64) -> f64 {
    let vis_a = visible_paint(top_a, h_a, view_top, view_bottom);
    let vis_b = visible_paint(top_b, h_b, view_top, view_bottom);
    if vis_a + vis_b <= f64::EPSILON {
        return 0.0;
    }
    (vis_b / (vis_a + vis_b)).clamp(0.0, 1.0)
}

fn visible_paint(top: f64, height: f64, view_top: f64, view_bottom: f64) -> f64 {
    (view_bottom.min(top + height) - view_top.max(top)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page alone on screen: nothing to blend toward.
    #[test]
    fn a_window_on_one_page_makes_no_progress() {
        let heights = [800.0, 800.0];
        // The window exactly covers page 1's paint.
        assert_eq!(blend_mix(&heights, 24.0, 0.0, 800.0, 1), 0.0);
        // Deep inside page 2 (dominant handed over): the window only sees
        // page 2, and page 3 is out of sight.
        assert_eq!(blend_mix(&heights, 24.0, 900.0, 800.0, 2), 0.0);
    }

    /// The load-bearing case: mid-transition, the progress is the next
    /// page's share of what is visible.
    #[test]
    fn the_next_pages_share_of_the_window_is_the_progress() {
        // Page 1 paints [0, 800], page 2 [824, 1624]. Window [700, 1500]:
        // page 1 shows 100px, page 2 shows 676px → 676/776.
        let heights = [800.0, 800.0];
        let mix = blend_mix(&heights, 24.0, 700.0, 800.0, 1);
        assert!((mix - 676.0 / 776.0).abs() < 1e-9, "{mix}");
    }

    /// Arriving at the next page is progress 1: the colour lands on the new
    /// page's own paper before the dominant tracker hands over.
    #[test]
    fn the_next_page_fully_in_view_is_progress_one() {
        // Window [900, 1700]: page 1 fully gone, page 2 spans it all.
        let heights = [800.0, 800.0];
        assert_eq!(blend_mix(&heights, 24.0, 900.0, 800.0, 1), 1.0);
    }

    /// The gap between pages is backdrop, not page paint: a window standing
    /// entirely in the gap holds the current colour instead of twitching.
    #[test]
    fn a_window_in_the_gap_holds_still() {
        // Gap spans [800, 824]; window [700, 800] leaves page 1 visible
        // alone → 0. A window entirely inside the gap sees neither page.
        let heights = [800.0, 800.0];
        assert_eq!(blend_mix(&heights, 24.0, 700.0, 100.0, 1), 0.0);
        assert_eq!(blend_mix(&heights, 24.0, 805.0, 10.0, 1), 0.0);
    }

    /// No Gap mode is the same math with the pages contiguous.
    #[test]
    fn zero_gap_blends_across_the_seam() {
        let heights = [800.0, 800.0];
        // Window [750, 1550]: page 1 shows 50px, page 2 shows 750px.
        let mix = blend_mix(&heights, 0.0, 750.0, 800.0, 1);
        assert!((mix - 750.0 / 800.0).abs() < 1e-9, "{mix}");
    }

    /// The last page, an unmeasured column, and a page past the end all
    /// hold still rather than guessing.
    #[test]
    fn the_ends_and_the_unmeasured_hold_still() {
        assert_eq!(blend_mix(&[800.0], 24.0, 0.0, 800.0, 1), 0.0);
        assert_eq!(blend_mix(&[], 24.0, 0.0, 800.0, 1), 0.0);
        assert_eq!(blend_mix(&[800.0, 800.0], 24.0, 0.0, 800.0, 3), 0.0);
    }

    /// Offsets count every preceding page's trailing gap: deep in a book the
    /// pair sits exactly where the strip laid it.
    #[test]
    fn offsets_count_the_gaps_above() {
        // Pages of 100 with gap 20: page 3 (idx 2) paints at 2×120 = 240,
        // page 4 at 360. Window [200, 400]: page 3 shows 100, page 4 shows
        // 40 → 40/140.
        let heights = [100.0; 5];
        let mix = blend_mix(&heights, 20.0, 200.0, 200.0, 3);
        assert!((mix - 40.0 / 140.0).abs() < 1e-9, "{mix}");
    }
}
