//! Reader-level reactive state, one file per domain: the document, the viewer
//! signals, the zoom pipeline's shape, search, the AI text selection and the
//! gloss marks. Pure UI chrome (sidebar, toast) lives in `state/ui` +
//! `state/app`; pure domain logic in `pdf-core`.
//!
//! This module is the barrel. Everything below used to be one 650-line file,
//! which meant a page that only wanted `ViewerSignals` read the document, the
//! search state and the gloss marks to find it, and any change to any of them
//! touched the same file. The six domains have nothing to say to each other
//! beyond the struct at the bottom that owns one of each.

pub mod ai;
pub mod document;
pub mod gloss;
pub mod search;
pub mod text;
pub mod viewer;
pub mod zoom;

use leptos::prelude::{Get, GetUntracked, Memo};

use pdf_core::appearance::TextureMode;
use pdf_core::layout::ViewMode;

// Only the names the app actually reaches for by their short path are
// re-exported; the rest are reached through their own module, which is the
// point of the split.
pub use ai::{AiSelectionState, SelectionDetail};
pub use document::{DEFAULT_PAGE_ASPECT, DocumentState, NO_DOCUMENT};
pub use gloss::GlossState;
pub use search::SearchState;
pub use text::TextDocState;
pub use viewer::{Motion, ViewerSignals};
pub use zoom::{ZoomCommand, ZoomTransition};

/// Page-host texture, provided via Leptos context by the app shell (derived
/// from settings). `PageCanvas` reads it to pick the `texture-*` class; the
/// reader never touches settings.
pub type TextureSignal = Memo<TextureMode>;

/// The reader's slice of app state: everything the PDF components and the
/// reader effects read/write. Sidebar/UI chrome is deliberately NOT here —
/// it is app chrome state, passed in explicitly where the reader needs it.
#[derive(Clone, Copy, Default)]
pub struct ReaderState {
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ai_selection: AiSelectionState,
    pub gloss: GlossState,
    /// The reflowable formats' document (blocks + the current page cut).
    /// Empty while a PDF — or nothing — is open.
    pub text: TextDocState,
}

impl ReaderState {
    /// True while a reflowable document is being read in the continuous
    /// stream — the one view mode whose reading is not paging. The chrome
    /// asks this before showing anything page-shaped (the indicator, the
    /// bottom bar's controls, the settings rows that would lie).
    pub fn text_streaming(&self) -> bool {
        self.document.format.get().is_text() && self.viewer.mode.get() == ViewMode::ScrollVertical
    }

    /// The stream's reading position as a rounded percentage of the whole
    /// document. Reads `scroll_top` and `container_size` TRACKED, so a
    /// derived signal around it updates with every scroll tick; the extent
    /// itself is read once, off the stream's own total (the scroll offset
    /// is the thing that moves, and it moves through `scroll_top`).
    pub fn stream_percent(&self) -> u32 {
        let top = self.viewer.scroll_top.get();
        let (_, viewport_h) = self.viewer.container_size.get();
        let total = self.text.stream_total.get();
        let extent = (total - viewport_h).max(1.0);
        ((top / extent) * 100.0).round().clamp(0.0, 100.0) as u32
    }

    /// The stream's reading position as 0..=1, or `None` while no stream is
    /// mounted. Purely a snapshot: persistence calls it inside its effect,
    /// where the tracked reads that matter (the page, the mode) already run.
    pub fn stream_fraction(&self) -> Option<f64> {
        let v = self.text.stream_handle()?;
        let total = v.total_size().get_untracked();
        let viewport = v.viewport().get_untracked().main;
        let offset = v.scroll_offset().get_untracked();
        let extent = (total - viewport).max(0.0);
        Some(if extent > 0.0 { (offset / extent).clamp(0.0, 1.0) } else { 0.0 })
    }
}
