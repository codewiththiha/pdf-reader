//! The page geometry every format publishes: how big each page is.
//!
//! This used to be the PDF half of the document, and the name said so
//! (`PdfContent`, reached as `content.pdf`). But nothing in it is PDF's. A page
//! size is known twice over — the intrinsic (scale-1) box the document declares,
//! and the CSS-px height the laid-out page actually took — and both drive things
//! that must not be asked about the format they are sizing: the strip
//! virtualizer seeds from them, the zoom coordinator anchors against them, the
//! blend backdrop reads the heights, and the thumbnails' row pitch is derived
//! from the first sheet.
//!
//! A PDF fills them from the file (`services::document::open::seed`, refined by
//! the engine's geometry callback as pages render). A reflowable document fills
//! them from its page cut, with A4 as the one fixed point
//! (`reflow::ReflowContent::apply_heights`) — which is exactly why the field
//! cannot keep a format's name: the reader's paged modes, its zoom ladder and
//! its progress chrome all read this and none of them may care who counted.
//!
//! `page1_size` is the answer every fixed-geometry surface uses before a page
//! has rendered, which is why the fallback policy sits on the document rather
//! than in each surface.

use leptos::prelude::*;

use pdf_engine::types::PageSize;

/// The open document's page sizes, at scale 1 and as laid out.
#[derive(Clone, Copy, Default)]
pub struct PageMetrics {
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Rendered CSS-px heights per page, seeded from `intrinsic` and refined
    /// by `on_geometry` as pages actually render.
    pub css_heights: RwSignal<Vec<f64>>,
}

impl PageMetrics {
    /// Back to "no pages". Called by [`super::DocumentState::reset`], and by
    /// nothing else: the two vectors must move together, or a strip lays out
    /// against heights from the book that was just closed.
    pub fn reset(&self) {
        self.page1_size.set(None);
        self.intrinsic.set(Vec::new());
        self.css_heights.set(Vec::new());
    }
}
