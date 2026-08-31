//! Serde types for search results returned by engine.search(), plus the pure
//! navigation/scroll maths the search UI runs on.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page: "next result" means the next match, which is usually still on the
//! current page. A match's rect is in scale-1 CSS px relative to its page's
//! top-left; the UI multiplies by the current scale to place it.

use serde::{Deserialize, Serialize};

/// One occurrence of the query.
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

/// One positioned text item of a page, as extracted by the engine: the
/// string plus its scale-1 CSS rect relative to the page's top-left. The
/// rect derivation from the pdf.js item transform stays engine-side (it is
/// shaped by pdf.js); everything downstream — the matching itself — is here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextItemGeo {
    pub s: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One page's text, ready to match: what the engine extracts per page and
/// hands to the wasm matcher (pdf_engine::wasm_ops registers it), and what
/// [`search_page`] returns matches for. The query is the already-lowercased,
/// trimmed string the engine searched for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageTextPayload {
    pub page: u32,
    pub query: String,
    pub items: Vec<TextItemGeo>,
}

/// Match one page's text against the query. Mirrors the engine's JS matcher
/// occurrence-for-occurrence: a case-insensitive scan per item, a match rect
/// interpolated inside the item's rect by character fraction, per-page
/// ordinals in reading order, and an ellipsised snippet of the surrounding
/// text.
///
/// Index semantics: the JS matcher walks UTF-16 code units; this walks bytes
/// of the lowercased string. For ASCII — which queries and PDF text are
/// overwhelmingly made of — the two agree exactly; beyond ASCII the
/// interpolated fraction and the snippet edges can differ by a character,
/// which shifts a highlight box by at most one glyph, never a page or a
/// count.
pub fn search_page(p: &PageTextPayload) -> Vec<SearchMatch> {
    let q = p.query.to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let qlen = q.len();
    let mut out = Vec::new();
    let mut ord: u32 = 0;
    for item in &p.items {
        if item.s.is_empty() || item.w <= 0.0 {
            continue;
        }
        let lower = item.s.to_lowercase();
        let len = lower.len();
        if len == 0 {
            continue;
        }
        let mut at = 0usize;
        while let Some(found) = lower[at..].find(&q) {
            let hit = at + found;
            out.push(SearchMatch {
                page: p.page,
                index: ord,
                text: snippet(&item.s, hit, qlen),
                x: item.x + item.w * hit as f64 / len as f64,
                y: item.y,
                w: (item.w * qlen as f64 / len as f64).max(1.0),
                h: item.h,
            });
            ord += 1;
            at = hit + qlen;
        }
    }
    out
}

/// Ellipsised context around the occurrence at byte offset `hit` (of the
/// lowercased text) with a query of `qlen` bytes — 25 bytes before, 30 after,
/// exactly the JS `snippetText` window. Slice edges are walked onto char
/// boundaries: JS can slice mid-code-unit, Rust must not.
fn snippet(s: &str, hit: usize, qlen: usize) -> String {
    let len = s.len();
    let start = floor_char_boundary(s, hit.saturating_sub(25));
    let end = ceil_char_boundary(s, (hit + qlen + 30).min(len));
    let pre = if start > 0 { "…" } else { "" };
    let post = if end < len { "…" } else { "" };
    format!("{pre}{}{post}", &s[start..end])
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
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

    // --- search_page (the ported matching core) ---------------------------

    fn geo(s: &str, x: f64, y: f64, w: f64, h: f64) -> TextItemGeo {
        TextItemGeo {
            s: s.to_string(),
            x,
            y,
            w,
            h,
        }
    }

    fn payload(page: u32, query: &str, items: Vec<TextItemGeo>) -> PageTextPayload {
        PageTextPayload {
            page,
            query: query.to_string(),
            items,
        }
    }

    /// Every occurrence is found in reading order, with the rect interpolated
    /// inside the item's rect and per-page ordinals — the values the UI's
    /// next/previous stepping and the highlight boxes are built from.
    #[test]
    fn finds_every_occurrence_with_interpolated_rects() {
        // "the theory of the mind": len 22, occurrences at 0, 4, 14.
        let p = payload(2, "the", vec![geo("the theory of the mind", 72.0, 140.4, 220.0, 12.0)]);
        let ms = search_page(&p);
        assert_eq!(ms.len(), 3, "{ms:?}");
        let want_x = [72.0, 112.0, 212.0];
        for (i, m) in ms.iter().enumerate() {
            assert_eq!(m.page, 2);
            assert_eq!(m.index, i as u32);
            assert_eq!(m.y, 140.4);
            assert_eq!(m.h, 12.0);
            assert!((m.x - want_x[i]).abs() < 1e-9, "x[{}]: {}", i, m.x);
            assert!((m.w - 30.0).abs() < 1e-9, "w[{}]: {}", i, m.w);
        }
    }

    /// Ordinals continue across items on the same page, and matching is
    /// case-insensitive while the snippet keeps the ORIGINAL casing.
    #[test]
    fn ordinals_span_items_and_matching_ignores_case() {
        let p = payload(
            5,
            "the",
            vec![
                geo("the theory of the mind", 0.0, 0.0, 220.0, 12.0),
                geo("THE quick brown fox", 0.0, 20.0, 180.0, 12.0),
            ],
        );
        let ms = search_page(&p);
        assert_eq!(ms.len(), 4, "{ms:?}");
        for (i, m) in ms.iter().enumerate() {
            assert_eq!(m.index, i as u32);
        }
        assert_eq!(ms[3].text, "THE quick brown fox");
        assert_eq!(ms[3].y, 20.0);
    }

    /// A hit in the middle of a long line carries both ellipses; a hit at the
    /// very start carries neither — the exact window the JS snippetText cut.
    #[test]
    fn snippets_window_twenty_five_before_and_thirty_after() {
        let line = format!("{}{}{}", "x".repeat(40), "find", "y".repeat(60));
        let p = payload(1, "find", vec![geo(&line, 0.0, 0.0, 1000.0, 10.0)]);
        let ms = search_page(&p);
        assert_eq!(ms.len(), 1);
        let want = format!("…{}find{}…", "x".repeat(25), "y".repeat(30));
        assert_eq!(ms[0].text, want);

        let head = format!("{}{}", "find", "y".repeat(60));
        let p = payload(1, "find", vec![geo(&head, 0.0, 0.0, 1000.0, 10.0)]);
        let ms = search_page(&p);
        assert_eq!(ms[0].text, format!("find{}", "y".repeat(30)));
    }

    /// Overlapping-step semantics: the scan resumes at hit + query length, so
    /// a self-overlapping haystack finds the same occurrences JS indexOf does.
    #[test]
    fn the_scan_steps_past_each_hit_like_indexof() {
        // "aaaa" searching "aa": JS finds 0 then 2 (indexOf(q, at + 2)) —
        // and NOT 1, because the scan steps past the whole hit.
        let p = payload(3, "aa", vec![geo("aaaa", 0.0, 0.0, 400.0, 10.0)]);
        let ms = search_page(&p);
        assert_eq!(ms.len(), 2, "{ms:?}");
        assert!((ms[0].x - 0.0).abs() < 1e-9);
        assert!((ms[1].x - 200.0).abs() < 1e-9, "second hit x: {}", ms[1].x);
        assert!((ms[1].w - 200.0).abs() < 1e-9, "w: {}", ms[1].w);

        // "aaa" searching "aa" therefore finds only the first.
        let p = payload(3, "aa", vec![geo("aaa", 0.0, 0.0, 300.0, 10.0)]);
        assert_eq!(search_page(&p).len(), 1);
    }

    /// Items without ink (zero width) and empty queries produce nothing; a
    /// match's width still never drops below one CSS px.
    #[test]
    fn skips_inkless_items_and_empty_queries() {
        let p = payload(1, "the", vec![geo("the", 0.0, 0.0, 0.0, 12.0)]);
        assert!(search_page(&p).is_empty());

        let p = payload(1, "", vec![geo("the", 0.0, 0.0, 220.0, 12.0)]);
        assert!(search_page(&p).is_empty());

        // A query wider than the item still yields a 1px-wide box.
        let p = payload(1, "the", vec![geo("the", 0.0, 0.0, 2.0, 12.0)]);
        let ms = search_page(&p);
        assert_eq!(ms.len(), 1);
        assert!(ms[0].w >= 1.0);
    }

    /// Multi-byte text must not panic on any slicing path: byte offsets from
    /// the lowercased scan are walked onto char boundaries for the snippet.
    #[test]
    fn multibyte_text_never_panics_and_still_matches() {
        let p = payload(
            7,
            "héllo",
            vec![geo("héllo wörld… héllo again", 0.0, 0.0, 500.0, 10.0)],
        );
        let ms = search_page(&p);
        assert_eq!(ms.len(), 2, "{ms:?}");
        for m in &ms {
            assert_eq!(m.page, 7);
            assert!(m.text.contains("héllo"));
            assert!(m.w >= 1.0);
        }
        assert_eq!(ms[0].index, 0);
        assert_eq!(ms[1].index, 1);
    }

    /// The payload is the wire contract with the engine's wasm matcher — pin
    /// the field names the JS side builds.
    #[test]
    fn page_text_payload_round_trips_through_serde() {
        let p = payload(
            9,
            "query",
            vec![geo("some text", 1.5, 2.5, 3.5, 4.5)],
        );
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"page\":9"), "{json}");
        assert!(json.contains("\"query\":\"query\""), "{json}");
        assert!(json.contains("\"s\":\"some text\""), "{json}");
        let back: PageTextPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
