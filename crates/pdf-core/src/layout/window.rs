//! Visible-window math: which grid rows overlap the viewport, and which
//! pages to keep mounted for the continuous render window.

use super::{DocumentLayout, RenderBudget};

pub fn visible_grid_rows(
    scroll_top: f64,
    viewport_h: f64,
    rows: usize,
    row_height: f64,
    buffer: usize,
) -> Option<(usize, usize)> {
    if rows == 0 || row_height <= 0.0 {
        return None;
    }
    let bottom = scroll_top + viewport_h.max(0.0);
    let grid_bottom = rows as f64 * row_height;
    // The viewport overlaps nothing: fully above or fully below the grid.
    if bottom < 0.0 || scroll_top >= grid_bottom {
        return None;
    }
    let mut first = (scroll_top / row_height).floor().max(0.0) as usize;
    let mut last = (bottom / row_height).floor().max(0.0) as usize;
    // Float safety: nudge the bottom boundary up a hair so a viewport ending
    // just short of a row boundary still renders that row.
    if bottom > scroll_top {
        last = ((bottom / row_height) + 1e-9).floor().max(0.0) as usize;
    }
    first = first.min(rows - 1);
    last = last.min(rows - 1);
    let first = first.saturating_sub(buffer);
    let last = (last + buffer).min(rows - 1);
    Some((first, last))
}

/// 0-based inclusive range of pages to KEEP MOUNTED: everything overlapping
/// `[scroll_top - look, scroll_top + viewport_h + look]` with
/// `look = budget.look_frac * viewport_h`, trimmed to `budget.max_items`.
/// Every partly-visible page is always kept; trimming drops the page furthest
/// from the viewport first, preferring the page below (reading direction).
pub fn render_range(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    budget: RenderBudget,
) -> Option<(usize, usize)> {
    DocumentLayout::new(heights, gap).window(scroll_top, viewport_h, budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::RenderBudget;

    const H: [f64; 3] = [100.0, 200.0, 100.0];


    #[test]
    fn render_range_expands_and_clamps() {
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        assert_eq!(render_range(124.0, 200.0, &H, 24.0, b), Some((0, 2)));
        // No look-ahead at all == exactly what is on screen.
        let none = RenderBudget { look_frac: 0.0, max_items: 7 };
        assert_eq!(render_range(124.0, 200.0, &H, 24.0, none), Some((1, 1)));
    }

    #[test]
    fn visible_grid_rows_windows() {
        // (scroll_top, viewport_h, rows, buffer, expected)
        let cases: &[(f64, f64, usize, usize, Option<(usize, usize)>)] = &[
            (0.0, 100.0, 3, 0, Some((0, 0))),
            (120.0, 200.0, 3, 0, Some((1, 2))),
            (0.0, 240.0, 3, 0, Some((0, 2))),
            (120.0, 200.0, 3, 1, Some((0, 2))),
            (240.0, 100.0, 3, 2, Some((0, 2))),
            // Exact boundaries: the row above has strictly scrolled out.
            (120.0, 100.0, 3, 0, Some((1, 1))),
            (240.0, 100.0, 3, 0, Some((2, 2))),
            // Zero-height viewport still resolves to the row under scroll_top.
            (50.0, 0.0, 3, 0, Some((0, 0))),
            (200.0, 0.0, 3, 0, Some((1, 1))),
            // No overlap: past the end, or entirely above the grid.
            (9999.0, 100.0, 3, 0, None),
            (-100.0, 50.0, 3, 0, None),
            // Single row, including its buffer clamp and its past-the-end case.
            (0.0, 100.0, 1, 0, Some((0, 0))),
            (50.0, 200.0, 1, 1, Some((0, 0))),
            (120.0, 100.0, 1, 0, None),
            // No rows at all.
            (0.0, 500.0, 0, 0, None),
            (0.0, 0.0, 0, 0, None),
        ];
        for &(st, vh, rows, buf, want) in cases {
            assert_eq!(
                visible_grid_rows(st, vh, rows, 120.0, buf),
                want,
                "st={st} vh={vh} rows={rows} buf={buf}"
            );
        }
    }
}


#[cfg(test)]
mod render_range_tests {
    use super::*;
    use crate::layout::{page_top_css, PAGE_GAP, RenderBudget, Strip};

    fn span_overlapping(
        top: f64,
        height: f64,
        heights: &[f64],
        gap: f64,
    ) -> Option<(usize, usize)> {
        Strip::new(heights.iter().copied(), gap)
            .overlapping(top, height)
            .map(|w| (w.first, w.last))
    }


    const GAP: f64 = PAGE_GAP;

    fn uniform(n: usize, h: f64) -> Vec<f64> {
        vec![h; n]
    }

    /// Park the viewport `frac` of the way down page `idx` (0-based).
    fn scroll_into(idx: usize, frac: f64, heights: &[f64], vh: f64) -> f64 {
        page_top_css(idx, heights, GAP) + heights[idx] * frac - vh * 0.5
    }

    /// THE POINT OF THE WHOLE CHANGE: mounted page count must fall as the
    /// pages grow past the viewport, instead of staying pinned at 2*buffer+1.
    #[test]
    fn zoomed_in_mounts_fewer_pages_than_zoomed_out() {
        let vh = 800.0;
        let b = RenderBudget::default();
        let count_at = |page_h: f64| {
            let h = uniform(40, page_h);
            let st = scroll_into(10, 0.5, &h, vh);
            let (f, l) = render_range(st, vh, &h, GAP, b).unwrap();
            l - f + 1
        };
        let zoomed_out = count_at(396.0); // 50%
        let normal = count_at(792.0); // 100%
        let zoomed = count_at(2376.0); // 300%
        let max_zoom = count_at(3960.0); // 500%

        assert!(
            zoomed_out >= normal && normal > zoomed && zoomed >= max_zoom,
            "expected monotonic shrink, got {zoomed_out} / {normal} / {zoomed} / {max_zoom}"
        );
        // Mid-page at 300%+ the neighbours are more than a screenful away.
        assert_eq!(max_zoom, 1, "at max zoom only the page under the eyes");
        // The old constant-buffer behaviour would have been 7 in every case.
        assert!(max_zoom < 7 && zoomed < 7);
    }

    /// The reader nearing the bottom of a tall page pulls the next one in
    /// before they reach it — the "render the one below at ~80%" behaviour,
    /// expressed as a distance rather than a hardcoded percentage.
    #[test]
    fn next_page_mounts_before_the_reader_arrives() {
        let vh = 800.0;
        let h = uniform(40, 3960.0); // 500%
        let b = RenderBudget::default();
        let idx = 10;

        // Mid-page: neighbours are far away, so just this page.
        let mid = scroll_into(idx, 0.5, &h, vh);
        assert_eq!(render_range(mid, vh, &h, GAP, b), Some((idx, idx)));

        // Scrolled so the page bottom is within one screenful: next is mounted
        // and ready, BEFORE any of it is on screen.
        let page_bottom = page_top_css(idx, &h, GAP) + h[idx];
        let near = page_bottom - vh - 10.0; // bottom is 10px beyond the viewport
        let (f, l) = render_range(near, vh, &h, GAP, b).unwrap();
        assert!(l >= idx + 1, "next page should be mounted early, got {f}..={l}");
        // ...and it is genuinely not visible yet, i.e. this is real read-ahead.
        let (vf, vl) = span_overlapping(near, vh, &h, GAP).unwrap();
        assert_eq!((vf, vl), (idx, idx), "next page must not be on screen yet");
    }

    /// Scrolling down evicts the page above once it is a screenful behind.
    #[test]
    fn page_above_is_dropped_once_far_enough_behind() {
        let vh = 800.0;
        let h = uniform(40, 3960.0);
        let b = RenderBudget::default();
        let idx = 10;
        let top = page_top_css(idx, &h, GAP);
        // Just past the top of page 10: page 9 is still within a screenful.
        let just_in = top + 100.0;
        let (f, _) = render_range(just_in, vh, &h, GAP, b).unwrap();
        assert_eq!(f, idx - 1, "the page just behind should still be warm");
        // Well into page 10: page 9 is now more than a screenful behind.
        let deep = top + vh * 1.5;
        let (f2, _) = render_range(deep, vh, &h, GAP, b).unwrap();
        assert_eq!(f2, idx, "the page behind should have been evicted");
    }

    /// INVARIANT: every visible page is always mounted, at any zoom, any
    /// budget — including budgets too small to hold them all.
    #[test]
    fn visible_pages_are_never_evicted() {
        let cases = [(396.0, 800.0), (792.0, 800.0), (3960.0, 420.0), (200.0, 1200.0)];
        for (page_h, vh) in cases {
            let h = uniform(30, page_h);
            for budget in [
                RenderBudget::default(),
                RenderBudget { look_frac: 0.0, max_items: 1 },
                RenderBudget { look_frac: 3.0, max_items: 2 },
            ] {
                for step in 0..40 {
                    let st = step as f64 * page_h * 0.37;
                    let Some((f, l)) = render_range(st, vh, &h, GAP, budget) else {
                        continue;
                    };
                    if let Some((vf, vl)) = span_overlapping(st, vh, &h, GAP) {
                        assert!(
                            f <= vf && l >= vl,
                            "page_h={page_h} vh={vh} st={st} budget={budget:?}: \
                             mounted {f}..={l} does not cover visible {vf}..={vl}"
                        );
                    }
                }
            }
        }
    }

    /// The ceiling is respected whenever it does not fight the invariant above.
    #[test]
    fn never_exceeds_the_ceiling_unless_visibility_demands_it() {
        let vh = 800.0;
        let h = uniform(60, 120.0); // many tiny pages: lots are visible at once
        for max_items in [1usize, 3, 5, 7, 12] {
            let b = RenderBudget { look_frac: 2.0, max_items };
            let st = 1000.0;
            let (f, l) = render_range(st, vh, &h, GAP, b).unwrap();
            let n = l - f + 1;
            let visible_n = span_overlapping(st, vh, &h, GAP)
                .map(|(a, b)| b - a + 1)
                .unwrap_or(0);
            assert!(
                n <= max_items.max(visible_n),
                "max_items={max_items}: mounted {n} pages (visible {visible_n})"
            );
        }
    }

    /// Degenerate inputs resolve to something sane rather than panicking.
    #[test]
    fn degenerate_inputs() {
        let b = RenderBudget::default();
        assert_eq!(render_range(0.0, 800.0, &[], GAP, b), None);
        // Zero-height viewport: still mounts the page under the scroll point.
        let h = uniform(5, 500.0);
        assert!(render_range(1100.0, 0.0, &h, GAP, b).is_some());
        // Past the end of the document: `None`, exactly as `visible_range`
        // answers, so the caller's existing "nothing to render" path is
        // reached rather than a surprise last-page mount.
        assert_eq!(render_range(99_999.0, 800.0, &h, GAP, b), None);
        assert_eq!(span_overlapping(99_999.0, 800.0, &h, GAP), None);
        // A zero max_items is treated as 1 rather than producing an empty range.
        let bad = RenderBudget { look_frac: 0.0, max_items: 0 };
        let (f, l) = render_range(1100.0, 800.0, &h, GAP, bad).unwrap();
        assert!(l >= f);
    }

    /// Parked in the gap between two pages: nothing is strictly visible, and
    /// the window must still resolve to the neighbouring pages.
    #[test]
    fn gap_parking_still_mounts_neighbours() {
        let h = [100.0, 200.0, 100.0];
        // 100..124 is the gap after page 0; a 15px viewport sits inside it.
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        let got = render_range(104.0, 15.0, &h, GAP, b);
        assert!(got.is_some(), "a gap position must still mount something");
        let (f, l) = got.unwrap();
        assert!(f == 0 && l >= 1, "expected the pages either side, got {f}..={l}");
    }
}
