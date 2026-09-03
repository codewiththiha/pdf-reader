//! One text page: an A4 host carrying its blocks as real DOM text.
//!
//! Where a PDF page stretches a raster, a text page lays its blocks out for
//! real — which is also why its zoom looks different under the hood. The
//! host is sized `A4 × scale` and the type inside scales by the SAME factor
//! (every size the typography owns is a scale-1 CSS variable on `<html>`
//! multiplied by the host's own `--ts`), so the layout is identical at
//! every scale and the crisp text is never a stretched bitmap. During a
//! zoom the host reflows frame by frame at the live display scale — cheap,
//! because only the mounted pages are in the DOM — while the PAGE CUT stays
//! put: it was computed at scale 1, and uniform scaling provably preserves
//! it, so pagination is never recomputed for a zoom.
//!
//! Paper treatment is the one place text is SIMPLER than PDF: a plain box
//! painted from `--color-paper` with the type set in `--color-ink` — the
//! very tokens the theme and tint pipelines already override — so base mode
//! and tint land on exactly the right colours without the `--canvas-filter`
//! or blend-mode machinery the always-light PDF rasters need.

use std::sync::Arc;

use leptos::prelude::*;

use pdf_core::appearance::TextureMode;
use text_core::page::{PAGE_HEIGHT, PAGE_WIDTH};
use text_core::typography::TextSettings;

use super::block::TextBlockView;
use crate::state::reader::TextDocState;
use crate::state::ReaderState;

/// The text typography, provided via context by the app bootstrap (derived
/// from settings) — the same pattern the appearance and texture signals use.
pub type TypographySignal = Memo<TextSettings>;

/// Where a page sits relative to the book spine while a book layout is on.
#[derive(Clone, Copy, Default, PartialEq)]
pub enum SpineSide {
    /// Derive the side from the page's parity — single pages and the scroll
    /// strip alternate recto/verso exactly like a bound book.
    #[default]
    Auto,
    /// Fixed LEFT of the spine (a spread's left-hand page): the gutter
    /// faces right.
    Left,
    /// Fixed RIGHT of the spine (a spread's right-hand page): the gutter
    /// faces left.
    Right,
}

/// The page's inline style at the live scale: the A4 box and its
/// book-layout (or symmetric) paddings. All paddings are geometry, scaled
/// here; the TYPE inside scales through `--ts` on the content column.
fn page_style(page: u32, scale: f64, settings: &TextSettings, spine: SpineSide) -> String {
    let geo = text_core::page::geometry(settings.book_layout);
    let (pad_left, pad_right) = match spine {
        SpineSide::Left => geo.spread_pads(settings.book_layout, false),
        SpineSide::Right => geo.spread_pads(settings.book_layout, true),
        SpineSide::Auto => geo.inline_pads(settings.book_layout, page.saturating_sub(1) as usize),
    };
    format!(
        "width:{}px;height:{}px;padding:{}px {}px {}px {}px;",
        PAGE_WIDTH * scale,
        PAGE_HEIGHT * scale,
        geo.pad_block * scale,
        pad_right * scale,
        geo.pad_block * scale,
        pad_left * scale,
    )
}

/// The content column's inline style: the one multiplier every typography
/// knob resolves through. The knobs themselves live as scale-1 custom
/// properties on `<html>` (painted by the typography effect); em-valued
/// ones ride the scaled font size on their own, px-valued ones multiply
/// `--ts` explicitly in the stylesheet.
pub(crate) fn content_style(scale: f64) -> String {
    format!("--ts:{};", scale)
}

#[component]
pub fn TextPage(
    /// 1-based page number this host renders.
    page: u32,
    state: ReaderState,
    /// Extra classes (the cross-axis `mx-auto` that centres a page and
    /// degrades to start-alignment on overflow, same as the PDF hosts).
    #[prop(default = String::new(), into)]
    class: String,
    /// The page texture mode, shared with the PDF hosts.
    #[prop(into)]
    texture: Signal<TextureMode>,
    /// Where this page sits relative to the book spine (see [`SpineSide`]).
    /// The spread fixes its two pages to the two sides; everywhere else the
    /// parity alternates on its own.
    #[prop(default = SpineSide::Auto)]
    spine: SpineSide,
) -> impl IntoView {
    let typography =
        use_context::<TypographySignal>().expect("TypographySignal must be provided by app bootstrap");
    let texture = Memo::new(move |_| texture.get());
    let host_class = move || {
        let t = texture.get();
        let base = if t == TextureMode::None {
            "tx-page".to_string()
        } else {
            format!("tx-page texture-{}", t.as_str())
        };
        if class.is_empty() {
            base
        } else {
            format!("{base} {class}")
        }
    };

    let scale = state.viewer.zoom.display.read_only();
    let text = state.text;
    // The block range is read TRACKED: a re-cut publishes a new range for
    // the same page number, and the host must re-render it.
    let range = move || page_range(text, page);

    view! {
        <div
            id=format!("tx-{page}-pg")
            class=host_class
            style=move || page_style(page, scale.get(), &typography.get(), spine)
        >
            <div
                class="tx-content"
                lang="en"
                style=move || content_style(scale.get())
            >
                <For
                    each=move || {
                        let doc_id = doc_id(text);
                        let (start, end) = range();
                        (start..end).map(|index| (doc_id, index)).collect::<Vec<(usize, usize)>>()
                    }
                    key=|(doc_id, index): &(usize, usize)| (*doc_id, *index)
                    children=move |(_, index): (usize, usize)| {
                        let block = block_at(text, index);
                        match block {
                            Some(block) => view! { <TextBlockView block=block /> }.into_any(),
                            // A re-cut can briefly hold a window from the
                            // outgoing pagination; an out-of-range index
                            // renders nothing rather than panicking.
                            None => ().into_any(),
                        }
                    }
                />
            </div>
        </div>
    }
}

/// The block range of `page`, read tracked (a re-cut republishes it).
fn page_range(text: TextDocState, page: u32) -> (usize, usize) {
    text.cuts.with(|cuts| {
        cuts.get(page.saturating_sub(1) as usize)
            .map(|cut| (cut.start, cut.end()))
            .unwrap_or((0, 0))
    })
}

/// One block by index, read tracked.
fn block_at(text: TextDocState, index: usize) -> Option<text_core::blocks::TextBlock> {
    text.doc.with(|doc| doc.as_ref().and_then(|d| d.blocks.get(index).cloned()))
}

/// The open document's identity, read tracked: the Arc pointer. Keying the
/// block list on it means a different document remounts its blocks instead
/// of reusing index keys the outgoing file already occupied.
fn doc_id(text: TextDocState) -> usize {
    text.doc
        .with(|doc| doc.as_ref().map_or(0, |d| Arc::as_ptr(d) as usize))
}
