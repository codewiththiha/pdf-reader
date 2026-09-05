//! Where a reflowable document's characters are, in the DOM.
//!
//! A page of pixels has a fixed grid, so anything painted over it — a gloss
//! stroke, a search hit — is placed by arithmetic on a stored rect. A document
//! the reader lays out itself has no grid: its blocks are re-cut by every
//! typography knob, and the only authority on where a word ended up is the
//! browser's own layout. This module is the one place that asks.
//!
//! The unit of address is the CHARACTER (a Unicode code point) counted over the
//! block row's text nodes in document order — what a gloss mark stores its spot
//! in, and what a search hit's occurrence ordinal counts. The DOM's UTF-16 code
//! units are converted to at the `set_start`/`set_end` boundary and nowhere
//! else, so an emoji is one character on both sides of that line.
//!
//! Two callers, one walk:
//!
//! * [`crate::components::ai::reflow_anchor`] projects a persisted mark's spot
//!   back to pixels, once per mark per refresh;
//! * [`crate::components::formats::reflow::highlight`] covers a block's row in
//!   search hits, once per row per invalidation.
//!
//! Both need the same three things — the row's text nodes, a `Range` over a span
//! of them, and that range's client rects — and both need the walk to skip the
//! layers painted OVER the text rather than being part of it.
//!
//! The arithmetic is pure and separate from the DOM walk so it is unit-testable
//! on the host: [`index_of_text_node`] (character offsets → a text node and an
//! offset inside it), [`clamp_span`] (a span against the text that is actually
//! there) and [`occurrence_spans`] (a query → the spans it covers).

use wasm_bindgen::JsCast;

/// Layers painted OVER a block's text, whose own text is not document text.
///
/// A gloss stroke's button carries the glossed word as its accessible name, and
/// a search hit's box is an empty sibling of the text it covers; counting either
/// would shift every offset after the first. The measure column renders every
/// block a second time and is skipped for the same reason — guarded rather than
/// excluded by construction, since it is mounted as a SIBLING of the page hosts
/// (`features/reader/page.rs`) and a walk that starts at a host never reaches it.
const OVERLAY_CLASSES: [&str; 3] = ["gloss-layer", "tx-hits", "tx-measure"];

/// Occurrences one row will report, mirroring the engine's per-page cap on the
/// boxes it paints (`MAX_HIGHLIGHTS_PER_PAGE` in `public/engine/highlights.ts`).
/// A one-character query in a long paragraph is the case this bounds; the two
/// pipelines keep the same number so a document reads the same either way.
pub(crate) const MAX_SPANS_PER_ROW: usize = 200;

/// The block's text nodes, in document order.
///
/// The walk is a plain `childNodes` recursion rather than a `TreeWalker`: it
/// needs no extra `web-sys` feature, and one block is a handful of nodes.
/// Nodes inside the block's own stroke layer are skipped — a mark's button
/// carries the glossed word as its accessible name, and counting that text
/// would shift every offset after the first mark.
pub(crate) fn text_nodes_of(el: &web_sys::Element) -> Vec<web_sys::Node> {
    let mut nodes = Vec::new();
    collect_text_nodes(el, &mut nodes);
    nodes
}

fn collect_text_nodes(node: &web_sys::Node, out: &mut Vec<web_sys::Node>) {
    match node.node_type() {
        web_sys::Node::TEXT_NODE => out.push(node.clone()),
        web_sys::Node::ELEMENT_NODE => {
            if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                let classes = el.class_list();
                // An overlay's own text is not document text: a mark's button
                // carries the glossed word as its accessible name, and counting
                // either one would shift every offset after it.
                //
                // The measure column is the same case, guarded rather than
                // excluded by construction: it renders every block a second
                // time, but it is mounted as a SIBLING of the page hosts
                // (`features/reader/page.rs`), so this walk — which starts at a
                // host — never reaches it. The class check is what keeps the
                // offsets honest if it is ever moved inside one; it costs one
                // `DOMTokenList::contains` per element.
                if OVERLAY_CLASSES.iter().any(|name| classes.contains(name)) {
                    return;
                }
            }
            let children = node.child_nodes();
            for index in 0..children.length() {
                if let Some(child) = children.item(index) {
                    collect_text_nodes(&child, out);
                }
            }
        }
        _ => {}
    }
}

/// Convert an offset counted in CHARACTERS (what a `ReflowSpot` stores, and
/// what the engine's tracker reports) into the UTF-16 code-unit offset a DOM
/// `Range` wants, within one text node's content.
///
/// The two units agree for everything in the Basic Multilingual Plane and
/// differ only for supplementary characters — emoji, mathematical
/// alphanumerics — where one character is two code units. Converting at this
/// one boundary is what lets the stored identity be honest characters on both
/// sides of the wire while the DOM still gets what it asked for.
fn utf16_offset_for_char(content: &str, char_offset: usize) -> u32 {
    content
        .chars()
        .take(char_offset)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

/// Which text node holds character `offset`, and how far into it that is.
///
/// Pure: `lengths` is a block's text nodes in order. An offset at or past the
/// end lands on the last node's end (or on `(0, 0)` for a block with no text),
/// so a mark whose document was edited shorter still projects onto something
/// sane instead of failing.
pub(crate) fn index_of_text_node(lengths: &[u32], offset: usize) -> (usize, u32) {
    let mut remaining = offset;
    for (index, &length) in lengths.iter().enumerate() {
        if remaining < length as usize {
            return (index, remaining as u32);
        }
        remaining -= length as usize;
    }
    match lengths.last() {
        Some(&last) => (lengths.len() - 1, last),
        None => (0, 0),
    }
}

/// A `[start, end)` span clamped into a block that now holds `chars`
/// characters. Ordered, so a clamped span can never come back backwards.
pub(crate) fn clamp_span(start: usize, end: usize, chars: usize) -> (usize, usize) {
    let start = start.min(chars);
    (start, end.clamp(start, chars))
}

/// A DOM `Range` over `[start, end)` of `el`'s text, clamped to what is
/// actually there. `None` when the block holds no text at all.
pub(crate) fn range_for_span(el: &web_sys::Element, start: usize, end: usize) -> Option<web_sys::Range> {
    let document = web_sys::window()?.document()?;
    let nodes = text_nodes_of(el);
    let texts: Vec<web_sys::Text> = nodes
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .cloned()
        .collect();
    // Character counts, because that is the unit a spot counts in; the DOM is
    // handed a code-unit offset only at the `set_start`/`set_end` boundary.
    let contents: Vec<String> = texts.iter().map(|text| text.data()).collect();
    let lengths: Vec<u32> = contents.iter().map(|c| c.chars().count() as u32).collect();
    let total: usize = lengths.iter().map(|&length| length as usize).sum();
    if total == 0 {
        return None;
    }
    let (start, end) = clamp_span(start, end, total);
    let (start_node, start_offset) = index_of_text_node(&lengths, start);
    let (end_node, end_offset) = index_of_text_node(&lengths, end);
    let range = document.create_range().ok()?;
    range
        .set_start(
            nodes.get(start_node)?,
            utf16_offset_for_char(contents.get(start_node)?, start_offset as usize),
        )
        .ok()?;
    range
        .set_end(
            nodes.get(end_node)?,
            utf16_offset_for_char(contents.get(end_node)?, end_offset as usize),
        )
        .ok()?;
    Some(range)
}

/// The client rects of `range`, as the pure tuples
/// [`union_box`](crate::components::ai::reflow_anchor::union_box) takes.
///
/// Both capture paths walk a selection's fragments through here — a reflowable
/// mark's spot projection and the PDF's
/// [`capture_selection`](crate::components::ai::anchor::pdf::capture_selection) —
/// so a multi-line selection is measured the same way whichever format it is in.
/// An empty list (a range the browser will not give rects for) unions to `None`,
/// which every caller already reads as "nothing to anchor to".
pub(crate) fn range_rects(range: &web_sys::Range) -> Vec<(f64, f64, f64, f64)> {
    let Some(rects) = range.get_client_rects() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rects.length() as usize);
    for index in 0..rects.length() {
        if let Some(rect) = rects.get(index) {
            out.push((rect.left(), rect.top(), rect.right(), rect.bottom()));
        }
    }
    out
}

/// The rendered text of a block row, one entry per text node, in document order.
///
/// Concatenated, this is the row's own coordinate system: the offsets
/// [`match_spans`] reports and [`range_for_span`] accepts are counts into it.
pub(crate) fn text_contents(el: &web_sys::Element) -> Vec<String> {
    text_nodes_of(el)
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .map(|text| text.data())
        .collect()
}

/// Every occurrence of `needle` in a block row's rendered text, as character
/// spans in the row's own coordinate system.
///
/// Case-insensitive and non-overlapping, which is `reflow_core::search`'s rule
/// for the hits it reports, so the nth span here is the nth hit the search found
/// in this block — the pairing a highlight box and an active match meet on.
pub(crate) fn match_spans(el: &web_sys::Element, needle: &str) -> Vec<(usize, usize)> {
    occurrence_spans(&text_contents(el).concat(), needle)
}

/// The pure half of [`match_spans`]: the character spans of every occurrence of
/// `needle` in `haystack`, in reading order.
pub(crate) fn occurrence_spans(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    // Case folding can change a string's LENGTH ('İ' folds to two characters),
    // and offsets into the folded copy would then not be the rendered text's.
    // When folding is not length-preserving the scan stays case-sensitive rather
    // than cover the wrong characters: a missed hit is a smaller lie than a
    // highlight over text the reader did not search for.
    let folded = haystack.to_lowercase();
    let (hay, needle) = if folded.chars().count() == haystack.chars().count() {
        (folded.as_str(), needle.to_lowercase())
    } else {
        (haystack, needle.to_string())
    };

    let chars: Vec<char> = hay.chars().collect();
    let want: Vec<char> = needle.chars().collect();
    if want.len() > chars.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + want.len() <= chars.len() {
        if chars[at..at + want.len()] == want[..] {
            out.push((at, at + want.len()));
            // Non-overlapping, like `str::match_indices`: "aa" in "aaa" is one
            // hit at 0 and a candidate at 1 that the first already consumed.
            at += want.len();
            if out.len() >= MAX_SPANS_PER_ROW {
                break;
            }
        } else {
            at += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_land_inside_their_own_text_node() {
        // Three text nodes: 5, 0 and 7 characters.
        let lengths = [5u32, 0, 7];
        assert_eq!(index_of_text_node(&lengths, 0), (0, 0));
        assert_eq!(index_of_text_node(&lengths, 4), (0, 4));
        // A zero-length node holds no characters, so nothing lands inside it:
        // the offset that would have is the next node's start.
        assert_eq!(index_of_text_node(&lengths, 5), (2, 0));
        assert_eq!(index_of_text_node(&lengths, 6), (2, 1));
        assert_eq!(index_of_text_node(&lengths, 11), (2, 6));
        // Past the end: the last node's end, not a panic and not a wrap.
        assert_eq!(index_of_text_node(&lengths, 12), (2, 7));
        assert_eq!(index_of_text_node(&lengths, 999), (2, 7));
    }

    #[test]
    fn an_empty_block_has_nowhere_to_put_an_offset() {
        assert_eq!(index_of_text_node(&[], 0), (0, 0));
        assert_eq!(index_of_text_node(&[], 40), (0, 0));
    }

    #[test]
    fn a_single_text_node_counts_from_its_own_start() {
        let lengths = [11u32];
        assert_eq!(index_of_text_node(&lengths, 0), (0, 0));
        assert_eq!(index_of_text_node(&lengths, 7), (0, 7));
        assert_eq!(index_of_text_node(&lengths, 11), (0, 11));
        assert_eq!(index_of_text_node(&lengths, 12), (0, 11));
    }

    #[test]
    fn character_offsets_convert_to_the_code_units_a_dom_range_wants() {
        // Plain ASCII: the two units agree, so nothing moves.
        assert_eq!(utf16_offset_for_char("palimpsest", 0), 0);
        assert_eq!(utf16_offset_for_char("palimpsest", 4), 4);
        // A supplementary character is ONE character and TWO code units, so
        // every offset after it shifts by one — the whole reason the spot is
        // stored in characters and converted here, at the DOM's boundary.
        let with_emoji = "ab\u{1F600}cd";
        assert_eq!(utf16_offset_for_char(with_emoji, 2), 2);
        assert_eq!(utf16_offset_for_char(with_emoji, 3), 4);
        assert_eq!(utf16_offset_for_char(with_emoji, 5), 6);
        // Past the end is the node's whole length: a clamped spot still
        // resolves to a real offset rather than throwing at `set_end`.
        assert_eq!(utf16_offset_for_char(with_emoji, 99), 6);
        assert_eq!(utf16_offset_for_char("", 3), 0);
    }

    #[test]
    fn spans_clamp_into_the_text_that_is_there() {
        assert_eq!(clamp_span(2, 8, 20), (2, 8));
        assert_eq!(clamp_span(2, 80, 20), (2, 20));
        assert_eq!(clamp_span(30, 80, 20), (20, 20));
        // A backwards span collapses; it never inverts.
        assert_eq!(clamp_span(9, 3, 20), (9, 9));
        assert_eq!(clamp_span(0, 0, 0), (0, 0));
    }

    #[test]
    fn occurrences_are_found_in_reading_order_and_do_not_overlap() {
        let spans = occurrence_spans("The Dune of Dune", "dune");
        assert_eq!(spans, vec![(4, 8), (12, 16)]);
        // One hit, not two: the first consumes the characters the second would
        // have started on.
        assert_eq!(occurrence_spans("aaa", "aa"), vec![(0, 2)]);
    }

    #[test]
    fn a_query_with_nothing_in_it_matches_nothing() {
        assert!(occurrence_spans("anything", "").is_empty());
        assert!(occurrence_spans("anything", "   ").is_empty());
        // A padded query still matches, at the trimmed needle's length.
        assert_eq!(occurrence_spans("a target here", " target "), vec![(2, 8)]);
    }

    #[test]
    fn spans_count_characters_not_bytes_or_code_units() {
        // An emoji is one character, so the hit after it starts at 6, not 7 or 8.
        let spans = occurrence_spans("ab\u{1F600}cd dune", "dune");
        assert_eq!(spans, vec![(6, 10)]);
        // Accented text folds to itself, and a case-insensitive scan still finds
        // it at the character offset the rendered text has.
        assert_eq!(occurrence_spans("héllo wörld", "HÉLLO"), vec![(0, 5)]);
    }

    #[test]
    fn a_long_run_of_hits_stops_at_the_cap() {
        let haystack = "a".repeat(MAX_SPANS_PER_ROW * 3);
        assert_eq!(occurrence_spans(&haystack, "a").len(), MAX_SPANS_PER_ROW);
    }
}
