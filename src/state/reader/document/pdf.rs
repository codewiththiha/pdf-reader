//! The PDF half of the open document: the geometry of its pages.
//!
//! A PDF's pages have a size the file dictates, known twice over — the
//! intrinsic (scale-1) box the document declares, and the CSS-px height the
//! rendered page actually took. Both are per-page vectors, and both drive
//! things that must not be asked about the format they are sizing: the strip
//! virtualizer seeds from them, the zoom coordinator anchors against them,
//! the blend backdrop reads the heights, and the thumbnails' row pitch is
//! derived from the first sheet.
//!
//! `page1_size` is the answer every fixed-geometry surface uses before a page
//! has rendered, which is why the fallback policy sits on the document rather
//! than in each surface.

use leptos::prelude::*;

use pdf_engine::types::PageSize;

/// The PDF pipeline's content: page sizes, at scale 1 and as laid out.
#[derive(Clone, Copy, Default)]
pub struct PdfContent {
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Rendered CSS-px heights per page, seeded from `intrinsic` and refined
    /// by `on_geometry` as pages actually render.
    pub css_heights: RwSignal<Vec<f64>>,
}

impl PdfContent {
    /// Back to "no pages". Called by [`super::DocumentState::reset`], and by
    /// nothing else: the two vectors must move together, or a strip lays out
    /// against heights from the book that was just closed.
    pub fn reset(&self) {
        self.page1_size.set(None);
        self.intrinsic.set(Vec::new());
        self.css_heights.set(Vec::new());
    }
}
