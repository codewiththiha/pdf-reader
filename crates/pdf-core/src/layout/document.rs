//! The cached column layout: a prebuilt [`Strip`] for the CURRENT page
//! heights, answering every scroll / zoom / search query through the same
//! prefix sums. Rebuild only when the heights change; the hot paths borrow.
//!
//! [`Strip`]: virtual_list::Strip

use super::RenderBudget;
use virtual_list::Strip;

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLayout {
    // `pub(crate)`: the anchoring math lives in the sibling `anchor` module
    // and reads these directly; the external API stays the methods below.
    pub(crate) strip: Strip,
    pub(crate) gap: f64,
}

impl DocumentLayout {
    pub fn new(heights: &[f64], gap: f64) -> Self {
        Self {
            strip: Strip::new(heights.iter().copied(), gap),
            gap,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.strip.is_empty()
    }

    /// Vertical offset of the top of page `page0` (0-based) within the column.
    pub fn page_top(&self, page0: usize) -> f64 {
        self.strip.offset(page0)
    }

    /// Height of page `i`, recovered from the prefix offsets (no per-call
    /// vector walk).
    pub fn height(&self, i: usize) -> f64 {
        if i + 1 < self.strip.len() {
            self.strip.offset(i + 1) - self.strip.offset(i) - self.gap
        } else {
            self.strip.total() - self.strip.offset(i)
        }
    }

    /// Total scrollable height of the whole column (pages + gaps).
    pub fn total(&self) -> f64 {
        self.strip.total()
    }



    /// 1-based page the reader is actually looking at (ties go to the lower
    /// page number). Falls back to 1 for an empty column.
    pub fn dominant(&self, scroll_top: f64, viewport_h: f64) -> u32 {
        self.strip.dominant(scroll_top, viewport_h) as u32 + 1
    }

    /// 0-based inclusive range of pages to keep mounted (see `render_range`).
    pub fn window(
        &self,
        scroll_top: f64,
        viewport_h: f64,
        budget: RenderBudget,
    ) -> Option<(usize, usize)> {
        self.strip
            .window(scroll_top, viewport_h, budget)
            .map(|w| (w.first, w.last))
    }
}

#[cfg(test)]
mod dominant_page_tests {
    use crate::layout::{anchored_scroll, dominant_page, page_top_css, PAGE_GAP};

    /// The basic contract: the page covering most of the viewport wins, a tall
    /// page filling it wins outright, and degenerate inputs fall back to
    /// `page_from_scroll` instead of guessing.
    #[test]
    fn most_visible_page_wins() {
        // Straddling the boundary at 1000 with an 800-tall viewport.
        let h = [1000.0, 1000.0];
        // scroll 700 -> page 1 covers 300, page 2 covers 476 (after the gap).
        assert_eq!(dominant_page(700.0, 800.0, &h, 24.0), 2);
        // scroll 400 -> page 1 covers 600, page 2 covers 176.
        assert_eq!(dominant_page(400.0, 800.0, &h, 24.0), 1);
        // One tall page fills the viewport outright (page 2 spans 2024..4024).
        assert_eq!(dominant_page(2500.0, 800.0, &[2000.0; 3], 24.0), 2);
        // Nothing measured -> 1. No viewport height yet -> top-edge answer.
        assert_eq!(dominant_page(0.0, 800.0, &[], 24.0), 1);
        assert_eq!(dominant_page(900.0, 0.0, &[800.0, 800.0], 24.0), 2);
    }

    /// A jump that aligns page P's top with the viewport top reports P, even
    /// when several shorter pages are visible below it.
    #[test]
    fn jump_to_page_top_reports_that_page() {
        let h = vec![400.0; 10];
        for target in 1..=8u32 {
            let top = page_top_css(target as usize - 1, &h, 24.0);
            assert_eq!(dominant_page(top, 800.0, &h, 24.0), target, "target {target}");
        }
    }

    /// Zooming out must not walk the counter: the reader holds still (the
    /// anchor is preserved by `anchored_scroll`), so the reported page must not
    /// change as everything shrinks.
    #[test]
    fn zoom_out_does_not_walk_the_counter() {
        let base = vec![800.0; 20];
        let (gap, vh) = (24.0, 752.0);
        // Reading page 11: park the viewport centre in the middle of it.
        let idx = 10usize;
        let mut st = page_top_css(idx, &base, gap) + base[idx] / 2.0 - vh / 2.0;
        let start = dominant_page(st, vh, &base, gap);
        assert_eq!(start, 11);
        let mut heights = base;
        for f in [0.93, 0.857, 0.833, 0.8] {
            st = anchored_scroll(st, vh, &heights, gap, f, vh * 0.5).unwrap();
            heights = heights.iter().map(|x| x * f).collect();
            assert_eq!(
                dominant_page(st, vh, &heights, gap),
                start,
                "counter drifted while zooming out"
            );
        }
    }

    /// Regression: a full zoom round-trip must land
    /// on the page it started on, in a document whose pages are NOT all the
    /// same height. This is the arithmetic half of the fix; the other half was
    /// a scroll write clamped by a not-yet-grown spacer, which lives in the DOM.
    #[test]
    fn zoom_round_trip_keeps_the_same_page_in_a_mixed_size_document() {
        // 300 pages: mostly letter, with legal / A4 / landscape mixed in — the
        // shape of a real book with plates and inserts.
        let intrinsic: Vec<f64> = (0..300)
            .map(|i| match i {
                _ if i % 37 == 0 => 612.0,
                _ if i % 13 == 0 => 842.0,
                _ if i % 7 == 0 => 1008.0,
                _ => 792.0,
            })
            .collect();
        let vh = 800.0;
        let anchor = vh * 0.5;
        let at = |scale: f64| -> Vec<f64> { intrinsic.iter().map(|h| h * scale).collect() };

        let mut scale = 1.0_f64;
        let mut heights = at(scale);
        // Park the viewport centre inside page 256.
        let mut scroll = page_top_css(255, &heights, PAGE_GAP) + heights[255] * 0.5 - anchor;
        let start_page = dominant_page(scroll, vh, &heights, PAGE_GAP);
        assert_eq!(start_page, 256, "test setup should start on page 256");

        // Out to 25%, back in to 175%, then home — the user's gesture.
        for target in [0.5_f64, 0.25, 0.5, 1.0, 1.75, 1.0] {
            let factor = target / scale;
            scroll = anchored_scroll(scroll, vh, &heights, PAGE_GAP, factor, anchor)
                .expect("anchored_scroll should produce a position");
            scale = target;
            heights = at(scale);
        }

        assert_eq!(
            dominant_page(scroll, vh, &heights, PAGE_GAP),
            start_page,
            "a zoom round-trip must not move the reader off their page"
        );
    }

    /// The height column must be derived from each page's OWN size. Seeding
    /// every page from page 1 mislocates every page below the first odd one.
    #[test]
    fn page_offsets_follow_each_pages_own_height() {
        let mixed = [792.0, 1008.0, 792.0, 842.0];
        assert_ne!(
            page_top_css(3, &mixed, PAGE_GAP),
            page_top_css(3, &[792.0; 4], PAGE_GAP),
            "uniform seeding hides real offsets in a mixed-size document"
        );
        assert_eq!(page_top_css(3, &mixed, PAGE_GAP), 792.0 + 1008.0 + 792.0 + 3.0 * PAGE_GAP);
    }
}
