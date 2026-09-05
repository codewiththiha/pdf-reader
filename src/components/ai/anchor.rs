//! Shared page-space anchor watchers: glue a [`PageAnchor`] to the live page
//! host so the selection Explain pill and the gloss card both follow scroll/zoom
//! and die when their origin leaves a configurable band of the viewport.
//!
//! The pure data type lives in `ai_core::gloss::PageAnchor` so state can hold
//! it without depending on the component layer.
//!
//! # Formats
//!
//! A page-space rect is the PDF's honest answer to "where is this word", and
//! it is only half an answer for a document that re-lays itself out: a
//! reflowable mark's durable identity is a block and a character range
//! ([`ReflowSpot`]), and its pixels have to be asked of the DOM again every
//! time anything moves. Rather than fork the watchers per format, this module
//! defines the two questions a format has to answer —
//! [`FormatAnchorBridge::screen_box`] and [`FormatAnchorBridge::capture`] —
//! and hands them down as one [`MarkResolver`] callback. The card, the pill,
//! the stroke layer, the spring and the persistence are all format-blind; a
//! new format adds a bridge and a mark-layer mount, and nothing else.

use ai_core::gloss::{mark_id, GlossBox, GlossMark, ReflowSpot};
use leptos::prelude::*;
use reader_core::view::ViewMode;
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::mark_layer::MARK_RADIUS;
use crate::components::ai::reflow_anchor::{range_rects, union_box};
use crate::dom_contract::{HOST_ATTR, HOST_PDF};
use crate::state::ReaderState;
use app_chrome::hooks::dom::by_id;
use app_chrome::hooks::use_viewport::viewport_size;
use app_chrome::hooks::use_raf::raf_coalesce;
use app_chrome::hooks::use_window_event::{add_window_capture_listener, use_window_event};

// Single public binding — do not also `use` PageAnchor above or rustc E0252s.
pub use ai_core::gloss::PageAnchor;

/// How an anchor's pixels are found right now. Returns the box in VIEWPORT CSS
/// px, or `None` when the anchor's host is not mounted (virtualized away) —
/// which every consumer already reads as "the anchor left the page".
///
/// The `f64` is the display scale. The PDF multiplies its stored page-space
/// rect by it; a reflowable format ignores it, because its type is scaled
/// through CSS custom properties and `getBoundingClientRect` already reports
/// the scaled result.
pub type MarkResolver = Callback<(PageAnchor, f64), Option<GlossBox>>;

/// The two questions a document format answers for the AI feature.
///
/// Both are about live layout, which is why the trait lives in the app and not
/// in `ai-core`: the crate keeps the serializable identity (a page and a rect,
/// a block and a character range) and stays free of the DOM and of Leptos.
pub trait FormatAnchorBridge {
    /// The screen-space box of an anchor right now; `None` if its host is
    /// unmounted.
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox>;
    /// Capture the current DOM selection as an anchor for this format.
    ///
    /// A format whose identity needs information the DOM alone cannot give
    /// (a reflowable mark needs the block index and the character offsets,
    /// which the engine's selection tracker walks out of the range) answers
    /// `None` here and is captured through its own path instead.
    fn capture(&self, scale: f64) -> Option<PageAnchor>;
}

/// The PDF's bridge: a page host's rect plus the mark's page-space rect, times
/// the display scale.
#[derive(Clone, Copy)]
pub struct PdfAnchorBridge {
    /// The view mode, which decides which host element carries the page.
    pub mode: ViewMode,
}

impl FormatAnchorBridge for PdfAnchorBridge {
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox> {
        screen_box(anchor, scale, self.mode)
    }

    fn capture(&self, scale: f64) -> Option<PageAnchor> {
        capture_selection(scale)
    }
}

/// The reflowable bridge (plain text and Markdown share it — they differ in
/// how a block is painted, never in where one lives).
///
/// The block-and-character identity rides in [`GlossMark::context`] as a
/// tagged envelope, so this bridge is reached with the mark in hand rather
/// than with a bare anchor: see [`crate::components::ai::reflow_anchor`].
#[derive(Clone, Copy)]
pub struct ReflowAnchorBridge {
    pub state: ReaderState,
    /// The spot to project. `None` for a mark captured before the tracker could
    /// walk its offsets (a legacy mark, or a selection whose block could not be
    /// identified), which then has nothing to project and says so.
    pub spot: Option<ReflowSpot>,
    /// The view mode, which says which host element carries the page.
    pub mode: ViewMode,
}

impl FormatAnchorBridge for ReflowAnchorBridge {
    fn screen_box(&self, _anchor: &PageAnchor, _scale: f64) -> Option<GlossBox> {
        // The spot IS the anchor for a document made of type. The box a mark was
        // captured with is a viewport snapshot: it is stale after one scroll, and
        // re-using it would move the card and the Explain pill onto whatever words
        // happen to be there now. So a spot that cannot be resolved — a block
        // virtualized away, one a re-parse orphaned, or an envelope from a
        // version this build cannot read — answers `None`, which is the same
        // thing a PDF says about an unmounted page, and the watchers already
        // treat it as "the origin left the viewport".
        let spot = self.spot?;
        super::reflow_anchor::spot_screen_box_in(self.state, &spot, self.mode)
    }

    fn capture(&self, _scale: f64) -> Option<PageAnchor> {
        // The engine's selection tracker normally hands the spot over with the
        // event, so this is the second path: the same walk, app-side, for a
        // selection that arrived without one. A reflowable bridge with a spot
        // already in hand has nothing to capture and says so.
        if self.spot.is_some() {
            return None;
        }
        super::reflow_anchor::capture_selection(self.state).map(|(_, anchor)| anchor)
    }
}

/// The screen box of one anchor, whichever format is open — the resolver the
/// card and the Explain pill watch through.
///
/// `spot` is the reflowable identity carried beside the anchor (a mark's
/// envelope, or the selection event's); it is ignored for a PDF, whose anchor
/// already is its identity. The view mode is read untracked: the watchers
/// subscribe to it themselves, so a mode flip already re-derives the box.
pub fn anchor_screen_box(
    state: ReaderState,
    anchor: &PageAnchor,
    spot: Option<ReflowSpot>,
    scale: f64,
) -> Option<GlossBox> {
    let mode = state.viewer.mode.get_untracked();
    if state.reflowable_untracked() {
        let bridge = ReflowAnchorBridge { state, spot, mode };
        return bridge.screen_box(anchor, scale);
    }
    let bridge = PdfAnchorBridge { mode };
    bridge.screen_box(anchor, scale)
}

/// Build the resolver a watcher should use: the format dispatch, decided per
/// call rather than per mount, because the document can change under a
/// component that outlives it. `spot_of` reads the reflowable identity of
/// whichever anchor is current (a mark's envelope, a selection's event detail).
pub fn anchor_resolver(state: ReaderState, spot: Signal<Option<ReflowSpot>>) -> MarkResolver {
    Callback::new(move |(anchor, scale): (PageAnchor, f64)| {
        // Read untracked: the watcher subscribes to the mark itself, and a
        // second subscription here would only re-derive the same box twice.
        let spot = spot.get_untracked();
        anchor_screen_box(state, &anchor, spot, scale)
    })
}

/// Build the resolver a stroke layer paints with: the same format dispatch as
/// [`anchor_resolver`], but answered in the LAYER's coordinates rather than the
/// viewport's, because a layer mounted inside a page host positions its strokes
/// against that host.
///
/// `page` is the host's own page, and the filter that keeps a page from
/// painting another page's marks; `None` is the continuous stream's
/// viewport-level layer, which has no page of its own and lets the resolver
/// decide what is on screen. `host_id` names the element those coordinates are
/// relative to (`None` for the viewport-level layer), and is resolved per call
/// rather than captured: the host may not be in the DOM yet when the layer is
/// constructed, and a remount replaces it.
pub fn stroke_resolver(
    state: ReaderState,
    page: Option<u32>,
    host_id: Option<String>,
) -> Callback<(GlossMark, f64), Option<GlossBox>> {
    Callback::new(move |(mark, scale): (GlossMark, f64)| {
        if state.reflowable_untracked() {
            let mode = state.viewer.mode.get_untracked();
            let host = host_id.as_deref().and_then(by_id);
            // A reflowable mark belongs to whichever page its block sits on
            // NOW, which a re-cut can move; the stored page number is the one
            // it was captured under.
            let spot = super::reflow_anchor::parse_spot(&mark.context);
            if let Some(page) = page {
                let current = spot
                    .and_then(|s| {
                        super::reflow_anchor::page_of_block(
                            state.document.content.reflow,
                            s.block,
                        )
                    })
                    .unwrap_or(mark.page);
                if current != page {
                    return None;
                }
            }
            return super::reflow_anchor::stroke_box(
                state,
                spot,
                mode,
                host.as_ref(),
                Some(mark.rect),
            );
        }
        if page.is_some_and(|page| mark.page != page) {
            return None;
        }
        // A PDF's rect is already host-local at scale 1, so its stroke is the
        // stored rect times the display scale — no DOM read, and no
        // round-trip through the viewport that would only subtract the host's
        // origin back off again.
        let rect = mark.rect;
        Some(GlossBox {
            x: rect.x * scale,
            y: rect.y * scale,
            w: rect.w * scale,
            h: rect.h * scale,
            r: rect.r,
        })
    })
}

/// What a stroke layer re-derives on.
///
/// A PDF's strokes are host-local at a fixed scale, so scale is the only thing
/// that moves them. A reflowable document's blocks are laid out by the browser:
/// the page cut, the typography and the scroll position all move a stroke, so
/// all three are in the fingerprint. It is a `u64` rather than the values
/// themselves so an unchanged re-measure notifies nothing.
pub fn layer_refresh(state: ReaderState) -> Signal<u64> {
    Signal::derive(move || {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        state.viewer.zoom.display.get().to_bits().hash(&mut hasher);
        if state.reflowable() {
            state.viewer.scroll_top.get().to_bits().hash(&mut hasher);
            state.viewer.container_size.get().0.to_bits().hash(&mut hasher);
            let _ = reflow_invalidation(state).get();
        }
        hasher.finish()
    })
}

/// An `invalidate` input for a watcher that has nothing beyond scroll, zoom and
/// the page number to react to — which is every PDF, since a page is fixed
/// pixels and cannot move under a mark.
pub fn no_invalidation() -> Signal<u64> {
    Signal::derive(|| 0u64)
}

/// The `invalidate` input a reflowable document needs: the page cut's
/// generation, the geometry it was cut with, and the stream's extent.
///
/// A re-cut moves blocks between pages, so a mark's page and its pixels both
/// change without anything scrolling or zooming. This is the signal that makes
/// the card and the Explain pill notice.
///
/// The typography is deliberately NOT read here. Every knob that moves type
/// moves the cut (the measure column re-publishes it), so the cut's generation
/// and `geometry` already cover it — and a knob that moves neither (the ink
/// dial, the column's alignment) cannot move a mark either. Reading settings
/// instead would re-derive every stroke on a colour change for nothing.
///
/// It is a FINGERPRINT rather than the vectors themselves: `Signal<u64>`
/// notifies only when the value differs, so a re-measure that re-cut nothing
/// costs one hash and wakes nobody.
///
/// The cut enters it as its GENERATION counter, not as its contents. This
/// derive re-runs on every scroll frame of a reflowable document (the stroke
/// layer reads it through [`layer_refresh`], which tracks scroll), and hashing
/// the split meant walking every page boundary of the open book once per frame
/// — hundreds of them in a long novel, to conclude, on almost every frame, that
/// nothing had moved. One counter read says the same thing. The counter's
/// granularity is coarser by design: it bumps on a re-publish that changed
/// nothing, and the cost of that is one wake of the consumers, which is less
/// than the hash it replaced.
pub fn reflow_invalidation(state: ReaderState) -> Signal<u64> {
    Signal::derive(move || {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        // `ViewMode` is `Eq` but not `Hash`, and its discriminant is all a
        // fingerprint needs.
        (state.viewer.mode.get() as u8).hash(&mut hasher);
        state.document.content.reflow.cut_generation.get().hash(&mut hasher);
        let geo = state.document.content.reflow.geometry.get();
        geo.content_width.to_bits().hash(&mut hasher);
        geo.content_height.to_bits().hash(&mut hasher);
        // The stream re-lays its blocks when the reading column's width moves
        // (a window resize, a page-margin change) without the page cut moving.
        state.document.content.reflow.stream_total.get().to_bits().hash(&mut hasher);
        state.viewer.container_size.get().0.to_bits().hash(&mut hasher);
        hasher.finish()
    })
}

/// The selection "Explain" pill lives until its origin fully leaves the viewport.
///
/// `1.0` is deliberate, not an untuned placeholder: the pill is small and
/// passive (it morphs nothing and owns no screen real estate), so it should
/// never vanish while any part of the text it points at is still visible —
/// unlike the gloss card below, which covers content and yields earlier.
pub const PILL_EXIT_FRAC: f64 = 1.0;
/// The expanded gloss card tolerates scroll until its origin passes this
/// fraction of the viewport height (or leaves the top edge).
pub const CARD_EXIT_FRAC: f64 = 0.8;

/// The host element a page lives in — the reader's page identity in the DOM.
///
/// Defined by the page host that paints it, and re-exported here because both
/// sides need the same string: the ids are one scheme shared by rasters and type,
/// which is what lets a selection anchor find a page of Markdown as readily as a
/// page of pixels.
pub use crate::components::viewer::page_host::host_id_for_mode;

fn page_from_host_id(id: &str) -> Option<u32> {
    if let Some(page) = id
        .strip_prefix("sp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    if let Some(page) = id
        .strip_prefix("dp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    if let Some(page) = id
        .strip_prefix("hp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    id.strip_prefix("cont-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
        .map(|index| index + 1)
}

/// Live viewport-space box for a page anchor. `None` when the scale is invalid
/// or the host page is not mounted (virtualized away) — which by itself counts
/// as "the anchor left the page".
pub fn screen_box(anchor: &PageAnchor, scale: f64, mode: ViewMode) -> Option<GlossBox> {
    if scale <= 0.0 {
        return None;
    }
    let hr = by_id(&host_id_for_mode(mode, anchor.page))?.get_bounding_client_rect();
    let h = anchor.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + anchor.rect.x * scale,
        y: hr.top() + anchor.rect.y * scale,
        w: anchor.rect.w * scale,
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// Whether an origin box has left the band it is allowed to live in: above the
/// viewport top, past `exit_frac` of the viewport height, or gone entirely
/// (its page unmounted, which by itself counts as left).
///
/// One definition for both bands — the watcher's soft `CARD_EXIT_FRAC` one and
/// the card's hard `1.0` one — so "off screen" cannot come to mean two
/// slightly different things.
pub fn origin_outside_band(origin: Option<GlossBox>, vh: f64, exit_frac: f64) -> bool {
    match origin {
        None => true,
        Some(b) => (b.y + b.h) < 0.0 || b.y > vh * exit_frac,
    }
}

/// Capture the current DOM selection as a page-space anchor, for a format whose
/// identity IS a page-space rect (the PDF).
///
/// The live selection's first range, and the element its start sits in.
///
/// Both capture paths begin here — this module's page-space one and
/// [`crate::components::ai::reflow_anchor::capture_selection`] — and differ only
/// in what they walk UP to afterwards: a page host for one, a block row for the
/// other. Sharing the walk is what keeps the half that is not format-specific
/// from drifting, and that half is where the awkward cases are: a collapsed
/// selection, an empty one, and a start container that is a text node rather
/// than an element.
pub(super) fn selection_start() -> Option<(web_sys::Range, web_sys::Element)> {
    let selection = web_sys::window()?.get_selection().ok()??;
    if selection.is_collapsed() || selection.range_count() == 0 {
        return None;
    }
    let range = selection.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    Some((range, el))
}

/// The page number comes from the host under the selection, not from the
/// reader's current-page signal. In the virtualized continuous reader those can
/// temporarily diverge, and anchoring to the signal can point at an unmounted
/// page host — which makes the floating Explain pill vanish even though the
/// selection itself is valid and visible.
///
/// The host is found through the `data-reader-host` attribute rather than a
/// `.pdf-page` class, so a format joins this path by tagging its host (see
/// [`crate::components::ai::reflow_anchor`]) and no selector here has to grow a
/// second class. A reflowable document is NOT captured by this function: its
/// anchor needs the block and character offsets the engine's selection tracker
/// reports with the event, so it goes through
/// [`crate::components::ai::reflow_anchor::anchor_of`] instead — which is what
/// `crate::effects::reader::selection_tracking` decides between.
pub fn capture_selection(scale: f64) -> Option<PageAnchor> {
    if scale <= 0.0 {
        return None;
    }
    let (range, el) = selection_start()?;
    let host = el
        .closest(&format!("[{HOST_ATTR}]"))
        .ok()
        .flatten()?;
    if host.get_attribute(HOST_ATTR).as_deref() != Some(HOST_PDF) {
        // Another format's host: its anchor is not a page-space rect, and
        // guessing one here would persist a mark that cannot be projected.
        return None;
    }
    let page = page_from_host_id(&host.id())?;
    let hr = host.get_bounding_client_rect();
    // One rect walk and one union rule for every format, so a stroke can never
    // be a different shape than the card that springs from it.
    let union = union_box(&range_rects(&range))?;
    Some(PageAnchor {
        page,
        rect: GlossBox {
            x: (union.x - hr.left()) / scale,
            y: (union.y - hr.top()) / scale,
            w: (union.w / scale).max(1.0),
            h: (union.h / scale).max(1.0),
            r: 0.0,
        },
    })
}

/// The one place a capture becomes a persisted mark.
///
/// The id scheme is `ai_core::gloss::mark_id`'s, and this is the only caller
/// that supplies it with a clock: the crate's gloss half is pure, so reading
/// the millisecond stamp belongs to the app side of the seam. Every capture
/// path goes through here — the Explain pill's click with an anchor in hand, its
/// fallback that walks the live range, and the reflowable one that has a spot
/// envelope to carry — so a mark cannot end up with an id no reader can
/// address, which is what three separately formatted `g{page}-{now}` literals
/// were one refactor away from.
pub fn captured_mark(
    word: impl Into<String>,
    context: impl Into<String>,
    anchor: PageAnchor,
) -> GlossMark {
    GlossMark {
        id: mark_id(anchor.page, js_sys::Date::now() as u64),
        word: word.into(),
        context: context.into(),
        anchor,
    }
}

/// The same capture, as a whole mark — the Explain pill's fallback when the
/// anchor it captured with its selection is gone.
pub fn capture_selection_mark(scale: f64, word: String, context: String) -> Option<GlossMark> {
    Some(captured_mark(word, context, capture_selection(scale)?))
}

#[derive(Clone, Copy)]
pub struct AnchorWatch {
    /// Live viewport-space box of the anchor (None = page not mounted).
    pub screen: RwSignal<Option<GlossBox>>,
    /// Origin left the allowed band: above the viewport top, or below
    /// `exit_frac` of the viewport height (or the page unmounted).
    pub exited: RwSignal<bool>,
    /// Synchronous re-derive (reads the DOM now). Call before using `screen`
    /// inside the same tick that the mark changed.
    pub refresh: Callback<()>,
}

/// Reusable "glued to the page, dies when the origin leaves" behaviour.
///
/// The screen box is re-derived whenever scroll / zoom / view mode / page /
/// container size change (plus a capture-phase scroll listener so *any*
/// scroller is caught, and window resize). `exit_frac` is the fraction of the
/// viewport height the origin may reach before `exited` flips: `1.0` means
/// "fully out of the viewport", `0.8` means "past 80% of the height".
///
/// `resolve` is the format's answer to "where is this anchor in the viewport
/// right now" — [`anchor_resolver`] builds the right one for whichever document
/// is open. `invalidate` is the format's answer to "something moved that scroll
/// and zoom do not cover": a reflowable document re-cuts its pages when the
/// typography or the column width changes, and a mark that stayed put through
/// that would be pointing at the wrong words. A PDF has nothing to add, so it
/// passes [`no_invalidation`].
pub fn watch_page_anchor(
    anchor: Signal<Option<PageAnchor>>,
    resolve: MarkResolver,
    scale: Signal<f64>,
    scroll_top: Signal<f64>,
    page: Signal<u32>,
    invalidate: Signal<u64>,
    exit_frac: f64,
) -> AnchorWatch {
    let screen = RwSignal::new(None::<GlossBox>);
    let exited = RwSignal::new(false);
    let tick = RwSignal::new(0u32);

    let refresh = Callback::new(move |_| {
        let b = anchor
            .get_untracked()
            .and_then(|a| resolve.run((a, scale.get_untracked())));
        if screen.get_untracked() != b {
            screen.set(b);
        }
        let (_, vh) = viewport_size();
        let out = origin_outside_band(b, vh, exit_frac);
        if exited.get_untracked() != out {
            exited.set(out);
        }
    });

    Effect::new(move |_| {
        let _ = anchor.get();
        let _ = scale.get();
        let _ = scroll_top.get();
        let _ = page.get();
        let _ = invalidate.get();
        let _ = tick.get();
        refresh.run(());
    });

    // Scroll and resize both fire faster than the screen updates, and each
    // re-derive reads layout twice (the page host's rect, the viewport size).
    // Coalescing to one recompute per frame drops the passes whose results
    // were overwritten before anything was painted; the card is spring-driven
    // at frame rate anyway, so it cannot tell the difference. Anything that
    // needs the anchor NOW (an open, mid-tick) calls `refresh` directly.
    let queue_refresh = raf_coalesce(move || tick.update(|n| *n += 1));
    let on_scroll = queue_refresh.clone();
    add_window_capture_listener("scroll", move |_| on_scroll());
    use_window_event("resize", move |_| queue_refresh());

    AnchorWatch {
        screen,
        exited,
        refresh,
    }
}

#[cfg(test)]
mod tests {
    use super::{CARD_EXIT_FRAC, GlossBox, PILL_EXIT_FRAC, origin_outside_band, page_from_host_id};

    fn origin(y: f64, h: f64) -> Option<GlossBox> {
        Some(GlossBox {
            x: 100.0,
            y,
            w: 40.0,
            h,
            r: 6.0,
        })
    }

    #[test]
    fn an_unmounted_page_is_outside_every_band() {
        assert!(origin_outside_band(None, 900.0, PILL_EXIT_FRAC));
        assert!(origin_outside_band(None, 900.0, CARD_EXIT_FRAC));
    }

    #[test]
    fn the_full_band_only_gives_up_off_screen() {
        let vh = 900.0;
        assert!(!origin_outside_band(origin(300.0, 100.0), vh, PILL_EXIT_FRAC));
        // Overlapping either edge is still visible.
        assert!(!origin_outside_band(origin(-50.0, 100.0), vh, PILL_EXIT_FRAC));
        assert!(!origin_outside_band(origin(850.0, 100.0), vh, PILL_EXIT_FRAC));
        // Fully above / fully below.
        assert!(origin_outside_band(origin(-150.0, 100.0), vh, PILL_EXIT_FRAC));
        assert!(origin_outside_band(origin(901.0, 100.0), vh, PILL_EXIT_FRAC));
    }

    #[test]
    fn the_card_band_gives_up_early() {
        let vh = 900.0; // the card's band ends at 720
        assert!(!origin_outside_band(origin(700.0, 20.0), vh, CARD_EXIT_FRAC));
        assert!(origin_outside_band(origin(760.0, 20.0), vh, CARD_EXIT_FRAC));
        // Still visible, but past the band: the pill would stay, the card goes.
        assert!(!origin_outside_band(origin(760.0, 20.0), vh, PILL_EXIT_FRAC));
    }

    #[test]
    fn parses_continuous_host_ids_into_one_based_pages() {
        assert_eq!(page_from_host_id("cont-0-pg"), Some(1));
        assert_eq!(page_from_host_id("cont-11-pg"), Some(12));
    }

    #[test]
    fn parses_single_page_host_ids() {
        assert_eq!(page_from_host_id("sp-1-pg"), Some(1));
        assert_eq!(page_from_host_id("sp-27-pg"), Some(27));
    }

    #[test]
    fn parses_dual_and_horizontal_host_ids() {
        assert_eq!(page_from_host_id("dp-3-pg"), Some(3));
        assert_eq!(page_from_host_id("hp-12-pg"), Some(12));
    }

    #[test]
    fn rejects_unrelated_ids() {
        assert_eq!(page_from_host_id("cont-wrap"), None);
        assert_eq!(page_from_host_id("page-3"), None);
    }
}
