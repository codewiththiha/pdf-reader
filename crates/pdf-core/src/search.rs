//! The search domain: the match-finding algorithm, the serde types the engine
//! answers with, and the pure navigation/scroll maths the search UI runs on.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page: "next result" means the next match, which is usually still on the
//! current page. A match's rect is in scale-1 CSS px relative to its page's
//! top-left; the UI multiplies by the current scale to place it.

use serde::{Deserialize, Serialize};

/// A match box, in scale-1 CSS px relative to its page's top-left. The
/// matcher's working type; `SearchMatch` carries the same four numbers flat,
/// because that is the engine's wire shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One occurrence of the query.
///
/// `x`/`y`/`w`/`h` are deliberately loose fields rather than a nested or
/// flattened rect: this struct is deserialized through `serde_wasm_bindgen`,
/// which cannot service `#[serde(flatten)]` (it needs `deserialize_any`,
/// which that data format does not implement). The flat shape is the wire
/// contract with `pdfEngine.js`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    /// 1-based page holding this occurrence.
    pub page: u32,
    /// Ordinal of this occurrence WITHIN its page, in reading order. The engine
    /// stamps the same number onto the highlight box it paints, so this pair
    /// names one box on screen without matching geometry.
    pub index: u32,
    /// Snippet of surrounding text, for the results list.
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `{ok:true, query, total, matches:[…]}` — engine.search().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: u32,
    pub matches: Vec<SearchMatch>,
}

// ---------------------------------------------------------------------------
// The match-finding algorithm
// ---------------------------------------------------------------------------
//
// This is the definition of record for what counts as a hit, where its box
// goes, and what the results list shows. `public/engine/search.ts` carries a
// transcription of it, because the engine is a separate bundle with no call
// path into this crate's wasm module — but the behaviour lives HERE, where it
// is testable, and the tests below are what the transcription is checked
// against.

/// One pdf.js text item, reduced to what the matcher needs.
///
/// `transform` is the item's text matrix `[a b c d e f]`: `hypot(c, d)` is the
/// font size and `(e, f)` the baseline origin in PDF user space (y up).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchTextItem {
    pub str: String,
    pub transform: [f64; 6],
    pub width: f64,
    pub height: f64,
}

/// Every hit on one page: the boxes the highlighter paints, and the entries
/// the results list shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchPageHit {
    pub rects: Vec<SearchRect>,
    pub matches: Vec<SearchMatch>,
}

/// Fraction of the font size treated as the ascent, for converting a PDF
/// baseline origin into a top-down box.
const ASCENT_RATIO: f64 = 0.8;

/// Characters of context kept on each side of a hit in the results list.
const SNIPPET_BEFORE: usize = 25;
const SNIPPET_AFTER: usize = 30;

/// A text item's box, flipped from PDF user space (origin bottom-left, y up)
/// into the reader's scale-1 CSS space (origin top-left, y down).
fn item_rect(item: &SearchTextItem, page_height: f64) -> SearchRect {
    let t = &item.transform;
    let font_size = (t[2].powi(2) + t[3].powi(2)).sqrt();
    let ascent = font_size * ASCENT_RATIO;
    SearchRect {
        x: t[4],
        y: page_height - t[5] - ascent,
        w: item.width,
        h: item.height,
    }
}

/// The results-list snippet for a hit starting at character offset `from` in
/// `text`. Clipped context gets an ellipsis; unclipped does not.
pub fn snippet(text: &str, query: &str, from: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let qlen = query.chars().count();
    let start = from.saturating_sub(SNIPPET_BEFORE);
    let end = (from + qlen + SNIPPET_AFTER).min(chars.len());
    let body: String = chars[start..end].iter().collect();
    let pre = if start > 0 { "…" } else { "" };
    let post = if end < chars.len() { "…" } else { "" };
    format!("{pre}{body}{post}")
}

/// Every occurrence of `query` on one page, in reading order.
///
/// `query` must already be lowercased and trimmed; matching is a plain
/// case-insensitive substring scan. Occurrences are non-overlapping — the
/// scan advances by the query length, so "aaa" searched in "aaaa" is one hit,
/// not two.
///
/// Hits do NOT span text items. pdf.js splits a line into items at its own
/// discretion (kerning, font runs, colour changes), and stitching them would
/// need per-item advance widths this input does not carry. A query broken
/// across that seam is not reported, which is the long-standing behaviour of
/// the reader and is worth knowing rather than discovering.
pub fn match_page(
    page: u32,
    page_height: f64,
    items: &[SearchTextItem],
    query: &str,
) -> SearchPageHit {
    let mut hit = SearchPageHit::default();
    if query.is_empty() {
        return hit;
    }
    let query_chars: Vec<char> = query.chars().collect();
    let qlen = query_chars.len();
    if qlen == 0 {
        return hit;
    }

    for item in items {
        if item.str.is_empty() {
            continue;
        }
        let r = item_rect(item, page_height);
        // A zero-width item has no place to draw a box, and dividing the
        // hit's offset across it would put the box anywhere. Written as a
        // positive test rather than `!(w > 0)` — clippy denies the negated
        // form on a partial order — which also drops NaN, since a width that
        // is not a number has no box either. Note this is NOT
        // `!w.is_sign_positive()`: IEEE calls +0.0 positive, so that spelling
        // would let the zero-width item through.
        if r.w <= 0.0 {
            continue;
        }
        let lower: Vec<char> = item
            .str
            .chars()
            .flat_map(|c| c.to_lowercase())
            .collect();
        // The proportional placement below assumes one char == one unit of
        // the item's width, so the denominator is the length actually
        // scanned.
        let len = lower.len().max(1) as f64;

        let mut at = 0usize;
        while at + qlen <= lower.len() {
            if lower[at..at + qlen] != query_chars[..] {
                at += 1;
                continue;
            }
            // `at` is an offset into the CASE-FOLDED text; for the snippet we
            // want the same offset in the original.
            let original_at = folded_offset_to_original(&item.str, at);
            // The hit's box is placed proportionally across the item: the
            // only geometry available is the item's own width, so the box
            // spans the fraction of it the query occupies.
            let x = r.x + (r.w * at as f64) / len;
            let w = ((r.w * qlen as f64) / len).max(1.0);
            hit.rects.push(SearchRect { x, y: r.y, w, h: r.h });
            hit.matches.push(SearchMatch {
                page,
                index: hit.matches.len() as u32,
                text: snippet(&item.str, query, original_at),
                x,
                y: r.y,
                w,
                h: r.h,
            });
            at += qlen;
        }
    }
    hit
}

/// Map an offset in `to_lowercase()`d text back to the same offset in the
/// original, for the characters that did not change length. Anything else
/// (a ligature that expands, a casing that folds to two chars) clamps to the
/// original's length — the snippet is context, not a contract.
fn folded_offset_to_original(original: &str, folded_at: usize) -> usize {
    let orig_len = original.chars().count();
    if orig_len == folded_at {
        return orig_len;
    }
    let mut folded_seen = 0usize;
    for (i, c) in original.chars().enumerate() {
        if folded_seen >= folded_at {
            return i;
        }
        folded_seen += c.to_lowercase().count();
    }
    orig_len
}

/// Next active-result index with wrap-around. `dir > 0` forward, `dir < 0` back.
/// `active = None` → first (dir > 0) or last (dir < 0).
pub fn next_search_index(len: usize, active: Option<usize>, dir: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match active {
        Some(i) if dir > 0 => (i + 1) % len,
        Some(i) if dir == 0 => i, // dir == 0 is a no-op: stay put
        Some(i) => (i + len - 1) % len,
        None if dir > 0 => 0,
        None if dir == 0 => return None, // dir == 0 with nothing active: no movement
        None => len - 1,
    })
}

/// Fraction of the reading area to leave above a match when scrolling it into
/// view, so it lands in comfortable reading position rather than jammed against
/// the top edge.
pub const MATCH_VIEW_BIAS: f64 = 0.35;

/// Scroll offset that brings a match into view, or `None` if it already is.
///
/// WHY A RANGE AND NOT A PAGE TOP. Jumping to `page_top` put the match anywhere
/// on the page — a hit near the bottom of a tall page landed off-screen, and
/// the reader had to hunt for it. This targets the MATCH.
///
/// It is also deliberately lazy: while the match is comfortably inside the
/// reading area the view does not move at all, so stepping through several hits
/// on one screen highlights them in place instead of jerking the page for each.
///
/// Arguments are in the scroll container's coordinates: `match_top`/`match_bot`
/// are the match's edges within the column, `scroll_top` the current offset,
/// `viewport_h` the container height, and `inset_top`/`inset_bottom` the parts
/// of the container hidden behind the floating toolbar / covered by the search
/// bar. `margin` keeps the match clear of those edges.
pub fn scroll_to_reveal(
    match_top: f64,
    match_bot: f64,
    scroll_top: f64,
    viewport_h: f64,
    inset_top: f64,
    inset_bottom: f64,
    margin: f64,
) -> Option<f64> {
    // The genuinely readable band, in scroll coordinates.
    let view_top = scroll_top + inset_top + margin;
    let view_bot = scroll_top + viewport_h - inset_bottom - margin;
    // A viewport too small for the insets (or a match taller than the band):
    // fall back to putting the match's top at the top of the readable area.
    if view_bot <= view_top || match_bot - match_top > view_bot - view_top {
        return Some((match_top - inset_top - margin).max(0.0));
    }
    if match_top >= view_top && match_bot <= view_bot {
        return None; // already comfortably visible — don't move
    }
    // Off-screen (or clipped): place it at the bias line, which reads better
    // than pinning it to whichever edge it left from.
    let band = view_bot - view_top;
    let target = match_top - inset_top - margin - band * MATCH_VIEW_BIAS;
    Some(target.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text item at a given baseline origin with a given font size, laid
    /// out left-to-right (the common case: `[size 0 0 size x y]`).
    fn item(text: &str, x: f64, y: f64, size: f64, width: f64) -> SearchTextItem {
        SearchTextItem {
            str: text.to_string(),
            transform: [size, 0.0, 0.0, size, x, y],
            width,
            height: size,
        }
    }

    // --- the matcher ------------------------------------------------------

    #[test]
    fn finds_one_hit_and_places_its_box() {
        let items = [item("The quick brown fox", 100.0, 700.0, 10.0, 190.0)];
        let hit = match_page(3, 800.0, &items, "quick");

        assert_eq!(hit.matches.len(), 1);
        let m = &hit.matches[0];
        assert_eq!(m.page, 3);
        assert_eq!(m.index, 0);
        assert_eq!(m.text, "The quick brown fox");

        // Box starts at the query's share of the item width: 4 of 19 chars
        // in, across a 190px item.
        let want_x = 100.0 + 190.0 * 4.0 / 19.0;
        assert!((m.x - want_x).abs() < 1e-9, "x = {}", m.x);
        let want_w = 190.0 * 5.0 / 19.0;
        assert!((m.w - want_w).abs() < 1e-9, "w = {}", m.w);
        // y is flipped: the baseline sits 700 up an 800-tall page, minus the
        // 0.8em ascent.
        let want_y = 800.0 - 700.0 - 10.0 * 0.8;
        assert!((m.y - want_y).abs() < 1e-9, "y = {}", m.y);
    }

    #[test]
    fn two_occurrences_in_one_item_get_ordinals_and_distinct_boxes() {
        let items = [item("cat and cat", 0.0, 100.0, 10.0, 110.0)];
        let hit = match_page(1, 200.0, &items, "cat");

        assert_eq!(hit.matches.len(), 2);
        assert_eq!(hit.matches[0].index, 0);
        assert_eq!(hit.matches[1].index, 1);
        assert!(
            hit.matches[1].x > hit.matches[0].x,
            "the second hit must sit to the right of the first"
        );
        // The highlighter paints from `rects`, and the two lists have to stay
        // index-aligned or box N lights up for match M.
        assert_eq!(hit.rects.len(), hit.matches.len());
        for (r, m) in hit.rects.iter().zip(hit.matches.iter()) {
            assert!((r.x - m.x).abs() < 1e-12 && (r.w - m.w).abs() < 1e-12);
        }
    }

    #[test]
    fn ordinals_run_across_items_in_reading_order() {
        let items = [
            item("first cat", 0.0, 100.0, 10.0, 90.0),
            item("second cat", 0.0, 80.0, 10.0, 100.0),
        ];
        let hit = match_page(1, 200.0, &items, "cat");
        assert_eq!(hit.matches.len(), 2);
        assert_eq!(hit.matches[0].index, 0);
        assert_eq!(hit.matches[1].index, 1);
        assert!(hit.matches[0].text.starts_with("first"));
        assert!(hit.matches[1].text.starts_with("second"));
    }

    #[test]
    fn matching_is_case_insensitive_but_the_snippet_keeps_the_original() {
        let items = [item("The QUICK Fox", 0.0, 100.0, 10.0, 130.0)];
        let hit = match_page(1, 200.0, &items, "quick");
        assert_eq!(hit.matches.len(), 1);
        assert!(
            hit.matches[0].text.contains("QUICK"),
            "snippet lost the document's casing: {}",
            hit.matches[0].text
        );
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        let items = [item("anything at all", 0.0, 100.0, 10.0, 100.0)];
        assert!(match_page(1, 200.0, &items, "").matches.is_empty());
    }

    #[test]
    fn a_zero_width_item_is_skipped() {
        // pdf.js emits zero-width items for some spacing runs. Dividing the
        // hit's offset across one would place the box anywhere, so the item
        // is dropped — its neighbour still matches.
        let items = [
            item("cat", 0.0, 100.0, 10.0, 0.0),
            item("cat", 0.0, 80.0, 10.0, 30.0),
        ];
        let hit = match_page(1, 200.0, &items, "cat");
        assert_eq!(hit.matches.len(), 1);
        assert!((hit.matches[0].y - (200.0 - 80.0 - 8.0)).abs() < 1e-9);
    }

    /// Overlapping occurrences advance by the query length: "aaa" in "aaaa"
    /// is one hit, not two. This is the engine's long-standing behaviour and
    /// the results list would double-count otherwise.
    #[test]
    fn occurrences_do_not_overlap() {
        let items = [item("aaaa", 0.0, 100.0, 10.0, 40.0)];
        assert_eq!(match_page(1, 200.0, &items, "aaa").matches.len(), 1);
        assert_eq!(match_page(1, 200.0, &items, "aa").matches.len(), 2);
    }

    /// A query broken across two text items is NOT reported. Documented
    /// rather than fixed: stitching items needs per-glyph advance widths the
    /// text content does not carry.
    #[test]
    fn hits_do_not_span_items() {
        let items = [
            item("quick bro", 0.0, 100.0, 10.0, 90.0),
            item("wn fox", 90.0, 100.0, 10.0, 60.0),
        ];
        assert!(match_page(1, 200.0, &items, "brown").matches.is_empty());
        // …while each half is found on its own.
        assert_eq!(match_page(1, 200.0, &items, "quick").matches.len(), 1);
    }

    /// The y flip is the easy thing to get backwards: a hit near the TOP of
    /// the PDF page (high y in user space) must land near y=0 in CSS space.
    #[test]
    fn the_y_axis_is_flipped_from_pdf_user_space() {
        let top = match_page(1, 800.0, &[item("cat", 0.0, 780.0, 10.0, 30.0)], "cat");
        let bottom = match_page(1, 800.0, &[item("cat", 0.0, 10.0, 10.0, 30.0)], "cat");
        assert!(
            top.matches[0].y < bottom.matches[0].y,
            "top-of-page hit ({}) must be above bottom-of-page hit ({})",
            top.matches[0].y,
            bottom.matches[0].y
        );
    }

    // --- snippets ---------------------------------------------------------

    #[test]
    fn a_short_snippet_is_not_ellipsised() {
        assert_eq!(snippet("The quick brown fox", "quick", 4), "The quick brown fox");
    }

    #[test]
    fn long_context_is_clipped_on_both_sides() {
        let text: String = (0..120).map(|i| char::from(b'a' + (i % 26))).collect();
        let s = snippet(&text, "q", 60);
        assert!(s.starts_with('…'), "{s}");
        assert!(s.ends_with('…'), "{s}");
        // 25 before + the query + 30 after.
        assert_eq!(s.chars().count(), 25 + 1 + 30 + 2);
    }

    #[test]
    fn a_hit_at_the_start_only_ellipsises_the_end() {
        let text: String = (0..120).map(|i| char::from(b'a' + (i % 26))).collect();
        let s = snippet(&text, "q", 0);
        assert!(!s.starts_with('…'), "{s}");
        assert!(s.ends_with('…'), "{s}");
    }

    // --- the pre-existing navigation maths --------------------------------

    /// Cycling through results: wrap in both directions, start at the first or
    /// last when nothing is active, and treat a single result as its own
    /// neighbour. `dir` carries only a sign, so a larger stride behaves the same.
    #[test]
    fn cycles_and_wraps() {
        // (len, active, dir, expected)
        let cases: &[(usize, Option<usize>, i32, Option<usize>)] = &[
            (3, Some(2), 1, Some(0)),
            (3, Some(0), -1, Some(2)),
            (3, Some(1), 1, Some(2)),
            (3, Some(1), -1, Some(0)),
            (3, Some(0), 1, Some(1)),
            (3, None, 1, Some(0)),
            (3, None, -1, Some(2)),
            (1, Some(0), 1, Some(0)),
            (1, Some(0), -1, Some(0)),
            (1, None, 1, Some(0)),
            (1, None, -1, Some(0)),
        ];
        for &(len, active, dir, want) in cases {
            assert_eq!(next_search_index(len, active, dir), want, "len={len} active={active:?} dir={dir}");
        }
    }

    /// No results means nothing to select, and `dir == 0` means stay put.
    #[test]
    fn empty_results_and_zero_direction() {
        assert_eq!(next_search_index(0, None, 1), None);
        assert_eq!(next_search_index(0, None, -1), None);
        assert_eq!(next_search_index(0, Some(0), 1), None);
        assert_eq!(next_search_index(3, Some(1), 0), Some(1));
        assert_eq!(next_search_index(1, Some(0), 0), Some(0));
        assert_eq!(next_search_index(3, None, 0), None);
    }

    /// A match already sitting in the readable band does not move the view.
    /// This is what lets several hits on one screen light up one after another
    /// without the page twitching.
    #[test]
    fn visible_match_does_not_scroll() {
        // scroll 600, viewport 800, top inset 48, bottom inset 56, margin 24
        // => readable band spans 672..1320 in scroll coordinates.
        assert_eq!(scroll_to_reveal(700.0, 720.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
        // Flush against each edge of the band is still "visible".
        assert_eq!(scroll_to_reveal(672.0, 700.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
        assert_eq!(scroll_to_reveal(1290.0, 1320.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
    }

    /// A match under the fold, or hidden behind the top chrome, is brought to
    /// the bias line — NOT to the edge it left from, and never past 0.
    #[test]
    fn offscreen_match_scrolls_to_the_bias_line() {
        let band = 800.0 - 48.0 - 56.0 - 2.0 * 24.0; // 624
        // Far below the fold.
        let want = 5000.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
        assert_eq!(
            scroll_to_reveal(5000.0, 5020.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            Some(want)
        );
        // Above the band (scrolled past): comes back to the same bias line.
        let want_up = 100.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
        assert_eq!(
            scroll_to_reveal(100.0, 120.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            Some(want_up.max(0.0))
        );
        // Near the very top of the document: clamped, never negative.
        assert!(scroll_to_reveal(10.0, 30.0, 600.0, 800.0, 48.0, 56.0, 24.0).unwrap() >= 0.0);
    }

    /// A match partly clipped by the bottom edge counts as not visible: the
    /// bug being fixed is precisely "the hit is on this page but off-screen".
    #[test]
    fn clipped_match_is_revealed() {
        // Band is 672..1320; this straddles the bottom edge, so the reader can
        // only see part of the hit.
        assert!(scroll_to_reveal(1300.0, 1360.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
        // And this one is clipped by the top chrome.
        assert!(scroll_to_reveal(650.0, 690.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
    }

    /// Degenerate geometry must still produce a usable offset rather than
    /// panicking or returning None: a match taller than the band, and a
    /// viewport smaller than its own insets.
    #[test]
    fn degenerate_geometry_falls_back_to_top_alignment() {
        assert_eq!(
            scroll_to_reveal(2000.0, 4000.0, 0.0, 800.0, 48.0, 56.0, 24.0),
            Some(2000.0 - 48.0 - 24.0)
        );
        assert_eq!(
            scroll_to_reveal(2000.0, 2020.0, 0.0, 60.0, 48.0, 56.0, 24.0),
            Some(2000.0 - 48.0 - 24.0)
        );
    }
}
