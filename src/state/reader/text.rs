//! The open text document: its blocks, and the page cut the reader is
//! currently reading them through.
//!
//! PDF keeps its content in the engine; a text document keeps its content
//! HERE — the whole file parsed into blocks the moment it opens. The
//! paginator (`text_core::pager`) packs those blocks into A4 pages, and the
//! result is published as three parallel signals the layouts read:
//!
//! * `heights` — every block's height at scale 1. Seeded by the pure
//!   estimate, replaced by the DOM's real measurement once the measure
//!   column has rendered once;
//! * `cuts` — the page split those heights produce;
//! * `block_page` — the inverse map (block → page), what navigation and
//!   search resolve positions through.
//!
//! The three are always written together ([`TextDocState::apply_heights`]),
//! because a split that disagrees with its map would send a page jump to
//! the wrong block.

use std::sync::Arc;

use leptos::prelude::*;

use text_core::blocks::TextBlock;
use text_core::page::{PageGeometry, PAGE_HEIGHT, PAGE_WIDTH};
use text_core::pager::{block_page_index, paginate, PageCut};
use text_core::typography::TextSettings;

/// One opened text document: its blocks. The format and the title the file
/// claimed live on `DocumentState` with every other document's identity.
pub struct TextDocument {
    pub blocks: Arc<Vec<TextBlock>>,
}

#[derive(Clone, Copy, Default)]
pub struct TextDocState {
    /// The open text document, or `None` while a PDF (or nothing) is open.
    pub doc: RwSignal<Option<Arc<TextDocument>>>,
    /// Block heights at scale 1 — estimate-seeded, measurement-refined.
    pub heights: RwSignal<Arc<Vec<f64>>>,
    /// The current page split of those heights.
    pub cuts: RwSignal<Arc<Vec<PageCut>>>,
    /// Block → 0-based page under the current split.
    pub block_page: RwSignal<Arc<Vec<u32>>>,
    /// The geometry the current split was cut with (a book-layout toggle
    /// re-cuts through the measure column).
    pub geometry: RwSignal<PageGeometry>,
    /// Bumped to force a re-measure (e.g. after fonts settle); the measure
    /// column tracks it alongside the typography.
    pub remeasure: RwSignal<u64>,
}

impl TextDocState {
    /// Back to the no-document state. Every field the open flow writes is
    /// reset here, so a field added to the struct cannot be silently
    /// forgotten by close.
    pub fn reset(&self) {
        self.doc.set(None);
        self.heights.set(Arc::new(Vec::new()));
        self.cuts.set(Arc::new(Vec::new()));
        self.block_page.set(Arc::new(Vec::new()));
        self.remeasure.set(0);
    }

    /// Publish a new set of block heights: re-cut the pages, publish the
    /// cut and its inverse map, and carry the document/viewer bookkeeping
    /// that hangs off the page count. Returns the page the reader lands on
    /// afterwards — the page holding the block the PREVIOUS cut's current
    /// page started on, so a re-cut never strands the position.
    pub fn apply_heights(
        &self,
        state: crate::state::AppState,
        heights: Vec<f64>,
        geo: PageGeometry,
    ) -> u32 {
        let cuts = paginate(&heights, geo.content_height);
        let block_count = state
            .reader
            .text
            .doc
            .with_untracked(|doc| doc.as_ref().map_or(0, |d| d.blocks.len()));
        let map = block_page_index(&cuts, block_count);

        // Where the reader was, in BLOCKS — survives the re-cut.
        let prev_page = state.reader.viewer.page.get_untracked();
        let anchor_block = self
            .cuts
            .with_untracked(|old| text_core::pager::first_block_of_page(old, prev_page));
        let new_page = map.get(anchor_block).map_or(1, |p| p + 1);

        let n = cuts.len() as u32;

        self.heights.set(Arc::new(heights));
        self.cuts.set(Arc::new(cuts));
        self.block_page.set(Arc::new(map));
        self.geometry.set(geo);

        // The shared page machinery: page count, per-page sizes (all A4 —
        // the cut's one fixed point) and the vertical strip's measurement
        // store, all at the live display scale.
        let scale = state.reader.viewer.zoom.visual_scale();
        state.reader.document.num_pages.set(n);
        state.reader.document.metrics.intrinsic.set(vec![
            pdf_engine::types::PageSize {
                width: PAGE_WIDTH,
                height: PAGE_HEIGHT
            };
            n as usize
        ]);
        state
            .reader
            .document
            .metrics
            .css_heights
            .set(vec![PAGE_HEIGHT * scale; n as usize]);

        new_page.clamp(1, n.max(1))
    }
}

/// The estimate's block metrics for a typography + page geometry — the
/// inputs of `text_core::pager::estimate_block_height`, gathered once per
/// pagination run.
pub fn estimate_metrics(settings: &TextSettings, geo: &PageGeometry) -> text_core::pager::BlockMetrics {
    text_core::pager::BlockMetrics {
        content_width: geo.content_width,
        font_size: settings.font_size,
        line_height: settings.line_height,
        paragraph_margin_em: settings.paragraph_margin,
        char_width: text_core::typography::body_char_width(settings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_metrics_carry_the_settings_through() {
        let mut s = TextSettings::default();
        s.font_size = 20.0;
        s.line_height = 1.5;
        s.paragraph_margin = 0.5;
        let geo = text_core::page::geometry(false);
        let m = estimate_metrics(&s, &geo);
        assert_eq!(m.font_size, 20.0);
        assert_eq!(m.line_height, 1.5);
        assert_eq!(m.paragraph_margin_em, 0.5);
        assert_eq!(m.content_width, geo.content_width);
        assert!(m.char_width > 0.0);
    }
}
