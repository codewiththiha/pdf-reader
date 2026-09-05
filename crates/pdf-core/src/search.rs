//! The PDF page-text index: the engine extracts, this crate searches.
//!
//! pdf.js can only extract text in the browser; everything after that —
//! lowercasing, occurrence matching, snippet building — is string work that
//! belongs here, off the JS heap. The engine hands pages over as [`PageText`]
//! (geometry already normalised to scale-1 CSS px), [`SearchIndex`] stores
//! them per page, and `query` scans the stored strings per keystroke with no
//! pdf.js round trip at all.
//!
//! The result shape ([`SearchMatch`] / [`SearchResponse`]) is format-agnostic
//! and lives in `reader_core::search`, together with the maths the result list
//! runs; a reflowable document searches its own blocks and answers in the same
//! shape, so the UI never learns which pipeline produced a hit.

use std::sync::Arc;

use reader_core::search::{SearchMatch, SearchResponse};

/// One extracted text run of a page, with its scale-1 rect.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    /// The original glyph string (what snippets read naturally).
    pub text: String,
    /// Lowercased copy of `text`, computed once at index build so every
    /// query scans strings instead of allocating a fresh lowercase per item.
    pub lower: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SearchItem {
    /// Build an index item, precomputing the lowercase copy.
    pub fn new(text: impl Into<String>, x: f64, y: f64, w: f64, h: f64) -> Self {
        let text = text.into();
        Self {
            lower: text.to_lowercase(),
            text,
            x,
            y,
            w,
            h,
        }
    }
}

/// One page's extracted text, as the engine hands it over.
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    /// 1-based page number.
    pub page: u32,
    pub items: Vec<SearchItem>,
}

/// The document's full-text index: every extracted page, in document order.
///
/// Pages can arrive OUT of order (the builder extracts concurrently), so the
/// index keeps them keyed by page and walks them sorted at query time.
#[derive(Debug, Default)]
pub struct SearchIndex {
    pages: std::collections::BTreeMap<u32, PageText>,
}

/// One query's worth of matched positions within a single item, safe against
/// non-UTF8-boundary slicing (ASCII is the byte==char fast path).
fn find_occurrences(hay: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    if hay.is_ascii() && needle.is_ascii() {
        // Byte offsets are char offsets here; mirrors JS `indexOf` loop.
        let mut out = Vec::new();
        let mut at = 0usize;
        while let Some(rel) = hay[at..].find(needle) {
            let pos = at + rel;
            out.push(pos);
            at = pos + needle.len();
        }
        return out;
    }
    // Non-ASCII: search on chars so offsets are character offsets and every
    // subsequent slice is a valid boundary.
    let hay: Vec<char> = hay.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Surrounding-text snippet: up to 25 chars before and 30 after the match,
/// with ellipses on the clipped sides — the same window the JS built.
fn snippet(text: &str, qlen: usize, at: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = at.saturating_sub(25);
    let end = (at + qlen + 30).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}

impl SearchIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of pages indexed so far.
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    /// True when nothing is indexed yet.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Add (or replace) one page's extracted text. Replacing keeps a
    /// re-extraction idempotent; the builder sends each page exactly once.
    pub fn add_page(&mut self, page: PageText) {
        self.pages.insert(page.page, page);
    }

    /// Drop everything (new document, rebuild).
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    /// Run `query` against the index, returning every occurrence in document
    /// order. An empty index or empty query yields an empty response.
    pub fn query(&self, query: &str) -> SearchResponse {
        let q = query.to_lowercase();
        let qlen = q.chars().count();
        let mut matches = Vec::new();
        if qlen == 0 {
            return SearchResponse {
                query: query.to_string(),
                total: 0,
                matches,
            };
        }
        for page in self.pages.values() {
            let mut ord = 0u32;
            for item in &page.items {
                // A zero-width run has no rectangle to highlight — skip it
                // exactly like the JS search did.
                if item.w <= 0.0 {
                    continue;
                }
                let len = item.lower.chars().count().max(1);
                for at in find_occurrences(&item.lower, &q) {
                    let at_f = at as f64;
                    matches.push(SearchMatch {
                        page: page.page,
                        index: ord,
                        text: Arc::<str>::from(snippet(&item.text, qlen, at)),
                        x: item.x + (item.w * at_f) / len as f64,
                        y: item.y,
                        w: (item.w * qlen as f64 / len as f64).max(1.0),
                        h: item.h,
                        // A page of pixels answers with the rect above; the
                        // block half is a reflowable document's.
                        block_hit: None,
                    });
                    ord += 1;
                }
            }
        }
        SearchResponse {
            query: query.to_string(),
            total: matches.len() as u32,
            matches,
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn item(text: &str, x: f64, w: f64) -> SearchItem {
        SearchItem::new(text, x, 100.0, w, 12.0)
    }

    fn page(n: u32, items: Vec<SearchItem>) -> PageText {
        PageText { page: n, items }
    }

    #[test]
    fn query_finds_every_occurrence_across_items_and_pages() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("The quick brown fox", 0.0, 100.0), item("jumps over the fox", 10.0, 90.0)]));
        index.add_page(page(2, vec![item("A fox in a box", 20.0, 80.0)]));
        let resp = index.query("fox");
        assert_eq!(resp.total, 3);
        // Document order, even though page 2 was added before page 1 here.
        assert_eq!(resp.matches[0].page, 1);
        assert_eq!(resp.matches[1].page, 1);
        assert_eq!(resp.matches[2].page, 2);
        // Ordinal is per page, in reading order.
        assert_eq!((resp.matches[0].page, resp.matches[0].index), (1, 0));
        assert_eq!((resp.matches[1].page, resp.matches[1].index), (1, 1));
        assert_eq!((resp.matches[2].page, resp.matches[2].index), (2, 0));
    }

    #[test]
    fn matching_is_case_insensitive_and_echoes_the_query() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("Hello World", 0.0, 100.0)]));
        let resp = index.query("WORLD");
        assert_eq!(resp.total, 1);
        assert_eq!(resp.query, "WORLD");
        assert_eq!(resp.matches[0].text.as_ref(), "Hello World");
    }

    #[test]
    fn rect_is_interpolated_across_the_item() {
        let mut index = SearchIndex::new();
        // "abcd", 100 px wide: "bc" starts at char 1 of 4 → x = 25.
        index.add_page(page(1, vec![item("abcd", 0.0, 100.0)]));
        let resp = index.query("bc");
        assert_eq!(resp.total, 1);
        assert!((resp.matches[0].x - 25.0).abs() < 1e-9, "x = {}", resp.matches[0].x);
        assert!((resp.matches[0].w - 50.0).abs() < 1e-9, "w = {}", resp.matches[0].w);
    }

    #[test]
    fn snippet_truncates_with_ellipses() {
        let mut index = SearchIndex::new();
        let long = format!("{}NEEDLE{}", "a".repeat(40), "b".repeat(40));
        index.add_page(page(1, vec![item(&long, 0.0, 100.0)]));
        let resp = index.query("needle");
        assert_eq!(resp.total, 1);
        let s = &resp.matches[0].text;
        assert!(s.starts_with('…') && s.ends_with('…'), "snippet: {s}");
        assert_eq!(s.chars().filter(|c| *c == 'N').count(), 1);
        // Window: ≤25 before (incl. ellipsis) and ≤30 after (incl. ellipsis).
        let core: String = s.chars().filter(|c| *c != '…').collect();
        assert!(core.chars().count() <= 25 + 6 + 30, "core len {}", core.chars().count());
    }

    #[test]
    fn whole_text_snippet_has_no_ellipses() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("a needle here", 0.0, 100.0)]));
        let resp = index.query("needle");
        assert_eq!(resp.matches[0].text.as_ref(), "a needle here");
    }

    #[test]
    fn empty_query_and_empty_index_are_clean() {
        let mut index = SearchIndex::new();
        assert_eq!(index.query("").total, 0);
        assert_eq!(index.query("x").total, 0);
        index.add_page(page(1, vec![item("alpha", 0.0, 1.0)]));
        assert_eq!(index.query("x").total, 0);
        assert_eq!(index.query("alpha").total, 1);
        index.clear();
        assert!(index.is_empty());
        assert_eq!(index.page_count(), 0);
    }

    #[test]
    fn zero_width_items_contribute_no_matches() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("noise", 0.0, 0.0)]));
        assert_eq!(index.query("noise").total, 0);
    }

    #[test]
    fn non_ascii_matching_never_panics_and_finds_the_query() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("héllo wörld", 0.0, 100.0)]));
        let resp = index.query("wörld");
        assert_eq!(resp.total, 1);
        assert_eq!(resp.matches[0].text.as_ref(), "héllo wörld");
        // And the casefold path: 'É' lowercases to 'é', which IS in "héllo".
        let resp = index.query("É");
        assert_eq!(resp.total, 1);
        let resp = index.query("héllo");
        assert_eq!(resp.total, 1);
    }

    #[test]
    fn overlapping_occurrences_advance_by_query_length() {
        // "aaaa" with query "aa": JS indexOf advances by qlen → 2 matches,
        // not 3. The port must keep that behaviour.
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("aaaa", 0.0, 100.0)]));
        let resp = index.query("aa");
        assert_eq!(resp.total, 2);
    }
}
