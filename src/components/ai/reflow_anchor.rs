//! Where a reflowable gloss mark lives, and how its pixels are found again.
//!
//! A PDF mark stores a rect against a page host and is done: the page is fixed
//! pixels, so the rect is the identity. A plain-text or Markdown document has
//! no such fixed grid — a font-size change, a window resize, the measure column
//! settling onto real heights, all of them re-cut the pages — so a page-space
//! rect would drift onto whatever text moved under it. The identity that
//! survives every re-flow is the BLOCK the words sit in and how far into that
//! block's rendered text they start ([`ReflowSpot`]).
//!
//! This module owns the two halves of that deal:
//!
//! * the ENVELOPE — a spot, and the sentence that was around it, serialized
//!   into [`GlossMark::context`] behind a version tag, so the persisted schema
//!   stays the one `PageAnchor` shape and a PDF's plain-sentence context can
//!   never be mistaken for a spot;
//! * the PROJECTION — `block + [start, end)` back to viewport pixels, by
//!   asking the DOM: block → page through the live `block_page` map, page →
//!   host element, then a real `Range` over the block's text nodes.
//!
//! Projection is deliberately never cached. It runs on the watcher's frame and
//! on the stroke layer's memo, both of which already re-run for scroll and
//! zoom, and a cached rect is exactly the thing a re-flow invalidates.
//!
//! The arithmetic is kept pure and separate from the DOM walk so it is
//! unit-testable on the host: [`index_of_text_node`] (character offsets → a
//! text node and an offset inside it), [`clamp_span`] (a span against the text
//! that is actually there) and [`union_box`] (client rects → one box).

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use leptos::prelude::*;
use reader_core::view::ViewMode;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use super::anchor::host_id_for_mode;
use super::gloss::mark_layer::MARK_RADIUS;
use crate::state::reader::ReflowContent;
use crate::state::ReaderState;

/// Version tag on the envelope in [`GlossMark::context`]. Bump it if the
/// payload's meaning changes; an old mark then simply reads as having no spot
/// and falls back to its stored rect rather than projecting wrongly.
const SPOT_TAG: &str = "rf1:";

/// The attribute every reader page host carries, naming the format family that
/// painted it. The engine's selection tracker and the capture below both find
/// their host through it, so a new format adds one attribute and joins.
pub const HOST_ATTR: &str = "data-reader-host";
// The host attribute holding the 1-based page that host paints is
// `data-host-page`. The hosts write it as a literal attribute and the engine's
// selection tracker reads it in TypeScript, so there is no Rust consumer to
// hang a constant on — this note is the cross-reference.
/// The host value a reflowable page or stream block carries.
pub const HOST_REFLOW: &str = "reflow";
/// The host value a PDF page carries.
pub const HOST_PDF: &str = "pdf";
/// On a rendered block: which block of the document it is, in document order.
/// This is the one handle a reflowable mark has on the DOM, and it is what
/// makes the paginated modes and the continuous stream resolve identically.
pub const BLOCK_INDEX_ATTR: &str = "data-block-index";

/// What a reflowable mark's `context` holds: the spot, and the sentence that
/// was around it when the mark was made.
///
/// The sentence has to ride along because `context` is the field the model is
/// handed to disambiguate the word (`ai_core::bridge::explain_word`), and a
/// mark is re-explained long after its selection is gone — from storage, from
/// a re-click on its stroke, after a restart. Storing the spot alone would
/// have meant sending the model a JSON envelope instead of prose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotEnvelope {
    /// The durable identity: block, and the character range inside it.
    pub spot: ReflowSpot,
    /// The surrounding sentence, as captured. Empty for a mark whose envelope
    /// predates this field, which then explains from the word alone.
    #[serde(default)]
    pub text: String,
}

/// A mark's `context` as the string it is persisted with. Not a `Display` impl:
/// this is a serialization format with a version tag, and reading it as text
/// would make the tag look decorative.
pub fn spot_envelope(spot: &ReflowSpot, sentence: &str) -> String {
    let payload = SpotEnvelope { spot: *spot, text: sentence.trim().to_string() };
    format!("{SPOT_TAG}{}", serde_json::to_string(&payload).unwrap_or_default())
}

/// The whole envelope a mark carries, if it carries one.
fn parse_envelope(context: &str) -> Option<SpotEnvelope> {
    let payload = context.strip_prefix(SPOT_TAG)?;
    serde_json::from_str(payload).ok()
}

/// The spot a mark carries, if it carries one.
///
/// A PDF's context is a sentence, which never starts with the tag, so this is
/// `None` for every PDF mark. For a reflowable one it is `None` only when the
/// mark predates spots (or its offsets could not be walked at capture), and
/// such a mark has nothing durable to be placed by — see
/// [`super::anchor::ReflowAnchorBridge`].
pub fn parse_spot(context: &str) -> Option<ReflowSpot> {
    parse_envelope(context).map(|envelope| envelope.spot)
}

/// The sentence to hand the model for a mark, whichever format made it: the
/// envelope's for a reflowable mark, the plain `context` for a PDF's.
///
/// An envelope with no sentence in it explains from the word alone rather than
/// falling back to the raw context, which is JSON and would only confuse the
/// model.
pub fn explain_context(mark: &ai_core::gloss::GlossMark) -> String {
    match parse_envelope(&mark.context) {
        Some(envelope) => envelope.text,
        None => mark.context.clone(),
    }
}

/// The page a block currently sits on, 1-based, or `None` before the document
/// has been paginated at all.
pub fn page_of_block(reflow: ReflowContent, block: usize) -> Option<u32> {
    reflow
        .block_page
        .with_untracked(|map| map.get(block).copied())
        .map(|page| page + 1)
}

/// The mounted element rendering `block`, or `None` when it is virtualized
/// away — the same answer a PDF gives for an unmounted page, with the same
/// consequence: the mark hides until the reader scrolls back to it.
///
/// The lookup is one attribute selector. It resolves inside the page host when
/// the block's page is mounted (the paginated modes, where scoping the query
/// keeps a stale row elsewhere in the document from answering), and falls back
/// to the whole document for the continuous stream, whose rows are not inside
/// any page host at all.
fn block_node(state: ReaderState, block: usize, mode: ViewMode) -> Option<web_sys::Element> {
    let reflow = state.document.content.reflow;
    let selector = format!("[{BLOCK_INDEX_ATTR}='{block}']");
    let page = page_of_block(reflow, block);
    if let Some(page) = page {
        if let Some(host) = app_chrome::hooks::dom::by_id(&host_id_for_mode(mode, page)) {
            if let Some(el) = host.query_selector(&selector).ok().flatten() {
                return Some(el);
            }
        }
    }
    web_sys::window()?
        .document()?
        .query_selector(&selector)
        .ok()
        .flatten()
}

/// The block's text nodes, in document order.
///
/// The walk is a plain `childNodes` recursion rather than a `TreeWalker`: it
/// needs no extra `web-sys` feature, and one block is a handful of nodes.
/// Nodes inside the block's own stroke layer are skipped — a mark's button
/// carries the glossed word as its accessible name, and counting that text
/// would shift every offset after the first mark.
fn text_nodes_of(el: &web_sys::Element) -> Vec<web_sys::Node> {
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
                // The stroke layer's own text is not document text (a mark's
                // button carries the glossed word as its accessible name), and
                // counting it would shift every offset after the first mark.
                //
                // The measure column is the same case, guarded rather than
                // excluded by construction: it renders every block a second
                // time, but it is mounted as a SIBLING of the page hosts
                // (`features/reader/page.rs`), so this walk — which starts at a
                // host — never reaches it. The class check is what keeps the
                // offsets honest if it is ever moved inside one; it costs one
                // `DOMTokenList::contains` per element.
                if classes.contains("glossLayer") || classes.contains("tx-measure") {
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

/// Convert an offset counted in CHARACTERS (what a [`ReflowSpot`] stores, and
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
pub fn index_of_text_node(lengths: &[u32], offset: usize) -> (usize, u32) {
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
pub fn clamp_span(start: usize, end: usize, chars: usize) -> (usize, usize) {
    let start = start.min(chars);
    (start, end.clamp(start, chars))
}

/// The viewport box a set of client rects covers, as the five fields a mark's
/// stroke is painted with.
///
/// Pure over `(left, top, right, bottom)` tuples, so the union and the radius
/// rule are testable without a DOM. Degenerate fragments — the zero-width rect
/// a `Range` reports at a line-box edge — are ignored, and an empty set yields
/// `None` rather than an infinite box.
pub fn union_box(rects: &[(f64, f64, f64, f64)]) -> Option<GlossBox> {
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let mut found = false;
    for &(l, t, r, b) in rects {
        if r - l <= 0.0 || b - t <= 0.0 {
            continue;
        }
        found = true;
        left = left.min(l);
        top = top.min(t);
        right = right.max(r);
        bottom = bottom.max(b);
    }
    if !found {
        return None;
    }
    let h = (bottom - top).max(1.0);
    Some(GlossBox {
        x: left,
        y: top,
        w: (right - left).max(1.0),
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// A DOM `Range` over `[start, end)` of `el`'s text, clamped to what is
/// actually there. `None` when the block holds no text at all.
fn range_for_span(el: &web_sys::Element, start: usize, end: usize) -> Option<web_sys::Range> {
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

/// The client rects of `range`, as the pure tuples [`union_box`] takes.
fn range_rects(range: &web_sys::Range) -> Vec<(f64, f64, f64, f64)> {
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

/// The viewport box a spot covers right now, in the reader's current view mode.
pub fn spot_screen_box(state: ReaderState, spot: &ReflowSpot) -> Option<GlossBox> {
    let mode = state.viewer.mode.get_untracked();
    spot_screen_box_in(state, spot, mode)
}

/// The viewport box a spot covers right now, or `None` when its block is not
/// mounted (or holds no text). This is the reflowable half of
/// [`super::anchor::anchor_screen_box`], and the whole reason a mark follows
/// its words across a re-pagination instead of staying where the pixels were.
///
/// The mode is passed in rather than read: a stroke layer is mounted per page
/// host and already knows which slot it is painting, and reading the viewer's
/// mode inside every mark's memo would subscribe the layer to a signal it has
/// no other use for.
pub fn spot_screen_box_in(
    state: ReaderState,
    spot: &ReflowSpot,
    mode: ViewMode,
) -> Option<GlossBox> {
    let el = block_node(state, spot.block, mode)?;
    let range = range_for_span(&el, spot.start, spot.end)?;
    union_box(&range_rects(&range))
}

/// A live selection's spot and the page it sits on, for a reflowable document.
///
/// The engine's tracker does the same walk in TypeScript and ships the spot
/// with the selection event, which is the path a normal selection takes: it
/// already has the range, and doing it once keeps the two from disagreeing.
/// This is the app-side capture, for the paths that need a spot without an
/// event to hand.
pub fn capture_selection(state: ReaderState) -> Option<(ReflowSpot, PageAnchor)> {
    let selection = web_sys::window()?.get_selection().ok()??;
    if selection.is_collapsed() || selection.range_count() == 0 {
        return None;
    }
    let range = selection.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    let row = el.closest(&format!("[{BLOCK_INDEX_ATTR}]")).ok().flatten()?;
    let block = row
        .get_attribute(BLOCK_INDEX_ATTR)
        .and_then(|value| value.parse::<usize>().ok())?;
    let spot = spot_of_range(&range, &row, block)?;
    Some((spot, anchor_of(state, &spot)?))
}

/// The spot a live range covers inside its block row: the characters before
/// the range's start, and the characters it spans.
///
/// Both are measured in the row's own rendered text (`textContent`), which is
/// the same coordinate system [`range_for_span`] walks later — and the reason
/// a Markdown mark stays put even though its source syntax is not rendered.
fn spot_of_range(range: &web_sys::Range, row: &web_sys::Element, block: usize) -> Option<ReflowSpot> {
    let total = row.text_content().unwrap_or_default().chars().count();
    if total == 0 {
        return None;
    }
    let before = range.clone_range();
    // `row` contains the range's start by construction (it is the ancestor the
    // start container was found through), so this cannot fail; the Result is
    // the DOM's, not a condition worth branching on.
    let _ = before.select_node_contents(row);
    before
        .set_end(&range.start_container().ok()?, range.start_offset().ok()?)
        .ok()?;
    // `Range::to_string` hands back the JS `String` object; the counts have to
    // be in CHARACTERS, and `JsString` is UTF-16, so the conversion through
    // `String` is what makes an emoji or a combining mark one character here
    // and one character in the engine's tracker too.
    let start = String::from(before.to_string()).chars().count();
    let span = String::from(range.to_string()).chars().count();
    let (start, end) = clamp_span(start, start + span, total);
    Some(ReflowSpot::new(block, start, end))
}

/// The anchor for a spot: the page it now sits on, plus the viewport box it
/// covers right now.
///
/// The rect is a FALLBACK, not the identity — the spot is. A reflowable document
/// has no durable page-space grid to store pixels against (that is the entire
/// reason the spot exists), so this is the box at the moment of capture. It is
/// read back only by a stroke layer serving a mark that carries no spot at all;
/// everything else re-derives from the spot. Dedup compares spots, never these
/// rects (see `crate::components::ai::gloss::controller::commands`).
pub fn anchor_of(state: ReaderState, spot: &ReflowSpot) -> Option<PageAnchor> {
    // A block the cut has not placed yet answers page 1 rather than nothing —
    // the same leniency a search hit gets (`effects::reader::search`), because
    // the box below is what actually locates the selection, and refusing an
    // anchor here would refuse the Info pill over text that is plainly on
    // screen. It happens only in the gap between a document opening and its
    // first measure pass.
    let page = page_of_block(state.document.content.reflow, spot.block).unwrap_or(1);
    let rect = spot_screen_box(state, spot)?;
    Some(PageAnchor { page, rect })
}

/// The box a reflowable stroke paints, in its own layer's coordinates.
///
/// A layer is always `position:absolute; inset:0` inside the element its
/// resolver measured, so the viewport box loses that element's origin here: a
/// `.tx-page` for a paginated mode, and the stream's scroller box for the
/// continuous one, where a single layer serves the whole reading column.
/// `host` is `None` only for a caller that wants the viewport box itself.
///
/// Neither case divides by the scale: a reflowable page's type is scaled
/// through CSS custom properties, so `getBoundingClientRect` already reports
/// the zoomed pixels, which is exactly what a stroke sitting over them needs.
///
/// `fallback` is the box the mark was captured with, kept for a mark that
/// carries no spot at all (one made before spots existed). It is a viewport
/// snapshot, so it is honest only while the layout has not moved — a mark whose
/// spot cannot be resolved hides instead, which is what the reader wants when it
/// is being asked where words are that are no longer there.
pub fn stroke_box(
    state: ReaderState,
    spot: Option<ReflowSpot>,
    mode: ViewMode,
    host: Option<&web_sys::Element>,
    fallback: Option<GlossBox>,
) -> Option<GlossBox> {
    let local = |b: GlossBox| match host {
        Some(host) => {
            let hr = host.get_bounding_client_rect();
            GlossBox { x: b.x - hr.left(), y: b.y - hr.top(), w: b.w, h: b.h, r: b.r }
        }
        None => b,
    };
    let Some(spot) = spot else {
        return fallback.map(local);
    };
    // A spot that cannot be resolved — a block virtualized away, or one a
    // re-parse orphaned — yields no stroke at all. That is not a dead mark: the
    // reader will scroll back, and the fallback box from capture time would
    // only paint a stroke over whatever text is there now.
    spot_screen_box_in(state, &spot, mode).map(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_round_trips_and_rejects_everything_else() {
        let spot = ReflowSpot::new(4, 12, 19);
        let envelope = spot_envelope(&spot, "  a manuscript page, scraped clean  ");
        assert!(envelope.starts_with(SPOT_TAG));
        assert_eq!(parse_spot(&envelope), Some(spot));

        // A PDF's context is a sentence, and must never read as a spot.
        assert_eq!(parse_spot("a manuscript page, scraped clean"), None);
        assert_eq!(parse_spot(""), None);
        // A tagged but corrupt payload is no spot, not a panic.
        assert_eq!(parse_spot("rf1:{\"spot\":}"), None);
        // An older envelope version is not this one's payload.
        assert_eq!(parse_spot("rf0:{\"block\":1,\"start\":0,\"end\":2}"), None);
    }

    #[test]
    fn the_envelope_carries_the_sentence_the_model_is_handed() {
        use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};

        let mark = |context: &str| GlossMark {
            id: "g1".to_string(),
            word: "palimpsest".to_string(),
            context: context.to_string(),
            anchor: PageAnchor { page: 1, rect: GlossBox::default() },
        };

        // Trimmed on the way in, so the envelope never stores the ragged edges
        // a double-clicked selection brings with it.
        let reflow = mark(&spot_envelope(&ReflowSpot::new(1, 0, 10), " scraped clean "));
        assert_eq!(explain_context(&reflow), "scraped clean");

        // A PDF's mark keeps its bare sentence.
        let pdf = mark("a manuscript page, scraped clean");
        assert_eq!(explain_context(&pdf), "a manuscript page, scraped clean");

        // An envelope from before the sentence travelled with it explains from
        // what is there rather than failing: `text` is `#[serde(default)]`.
        let legacy = mark("rf1:{\"spot\":{\"block\":1,\"start\":0,\"end\":2}}");
        assert_eq!(parse_spot(&legacy.context), Some(ReflowSpot::new(1, 0, 2)));
        assert_eq!(explain_context(&legacy), "");
    }

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
    fn the_union_covers_every_fragment_and_ignores_degenerate_ones() {
        let rects = [
            (100.0, 10.0, 150.0, 24.0),
            // A zero-width fragment at a line-box edge contributes nothing.
            (150.0, 10.0, 150.0, 24.0),
            (20.0, 26.0, 90.0, 40.0),
        ];
        let union = union_box(&rects).expect("a real fragment");
        assert_eq!((union.x, union.y), (20.0, 10.0));
        assert_eq!((union.w, union.h), (130.0, 30.0));
        // The stroke's radius rule, shared with the PDF's page-space rects.
        assert_eq!(union.r, MARK_RADIUS.min(union.h / 2.0));
    }

    #[test]
    fn a_selection_of_only_degenerate_fragments_projects_to_nothing() {
        assert!(union_box(&[]).is_none());
        assert!(union_box(&[(5.0, 5.0, 5.0, 5.0)]).is_none());
        assert!(union_box(&[(5.0, 5.0, 9.0, 5.0)]).is_none());
    }

    #[test]
    fn a_hairline_fragment_still_gets_a_stroke_worth_of_box() {
        let union = union_box(&[(10.0, 10.0, 10.4, 11.0)]).expect("non-degenerate");
        assert!(union.w >= 1.0 && union.h >= 1.0);
        assert_eq!(union.r, MARK_RADIUS.min(union.h / 2.0));
    }
}
