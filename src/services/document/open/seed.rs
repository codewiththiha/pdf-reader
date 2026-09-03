//! Seeding the app state for a freshly opened document.
//!
//! One synchronous batch, in a deliberate order, run while the status is
//! still `Opening` and nothing is mounted. The order is the interesting part
//! and each step says why it is where it is.

use std::sync::Arc;

use leptos::prelude::*;

use pdf_core::filename::display_name;
use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::math::{clamp_scale, fit_scale, FitMode};
use pdf_engine::types::{OpenResult, PageSize};

use crate::state::AppState;

/// What the rest of the flow needs to know once the state is seeded.
pub(super) struct Seeded {
    /// The document's display name, for the shelf record.
    pub name: Option<String>,
    /// The page to resume at, clamped to the book that actually opened.
    pub resume: u32,
    pub num_pages: u32,
}

/// Write everything the fresh mount will read. Returns the resume point,
/// which the caller jumps to once the view exists.
pub(super) fn seed(state: AppState, path: &str, open: OpenResult, saved_page: u32) -> Seeded {
    let page1 = open.page1_size;
    let num_pages = open.num_pages;
    let name = display_name(open.title.as_deref(), Some(path));
    // Document state.
    state.reader.document.num_pages.set(num_pages);
    // The paper session resets for the new book and asks the engine's
    // per-document cache for its colour — synchronously, while the status is
    // still `Opening` and nothing is mounted, so a cache hit repaints the
    // blend backdrop with the intended colour in the reader's very first
    // frame (zero sampling work) instead of flashing the theme paper first.
    pdf_engine::paper::document_open(path, num_pages);
    state
        .reader
        .document
        .metrics
        .intrinsic
        .set(intrinsic_sizes(&open.page_widths, &open.page_heights, &page1, num_pages));
    state.reader.document.title.set(open.title);
    state.reader.document.author.set(open.author);
    // The previous book's chapters must not linger while the new tree
    // resolves (a mid-read open never passes through close_document's reset).
    // The engine's `open` no longer resolves the outline at all — see
    // `super::outline`.
    state.reader.document.outline.set(Arc::new(Vec::new()));
    state.reader.document.outline_pending.set(true);
    state.reader.document.page1_size.set(Some(page1.clone()));
    state.reader.document.path.set(Some(path.to_string()));

    // Gloss highlights for THIS document. Loaded here rather than lazily by
    // the mark layer so the very first page mount already paints them (they
    // are page-space rects, not DOM state). `reset` first so a field added to
    // `GlossState` cannot be missed here; the loaded marks then overwrite the
    // empty list.
    state.reader.gloss.reset();
    state.reader.gloss.marks.set(
        crate::storage::load_gloss()
            .remove(path)
            .unwrap_or_default(),
    );

    // Resume point (clamped to the real count AND at least page 1 — a
    // re-edited document may have fewer pages than remembered, and a
    // stale/transient saved 0 must never resume before the book).
    let resume = saved_page.clamp(1, num_pages.max(1));

    // Fresh-open baseline: page 1, top of the column. The resume jump happens
    // AFTER the view mounts (see the caller), because writing `page = resume`
    // here — in the same batch as the `page_heights` reset and
    // `scroll_top = 0` — races the page-tracking effects: the scroll→page
    // effect reads scroll 0 and "corrects" the page back to 1 before the jump
    // lands.
    //
    // ALL of this lands BEFORE `status = Ready` flips the route to the
    // reader: the mount-time container-bind scroll reads `viewer.page`, and a
    // stale `page = 42` from the document that was open a drag-and-drop ago
    // would jump the new book's strip to its page 42 for the frames between
    // the flip and this correction. Baseline first, mount second.
    state.reader.viewer.page.set(1);
    state.reader.viewer.scroll_top.set(0.0);
    // The startup fit mode is a user setting (Fit Page / Fit Width), not a
    // hard-coded fit-width. `sanitize` has already replaced a persisted `None`
    // with the default, so this is always a real fit mode here.
    let startup_fit = state.settings.with(|s| s.layout.default_fit);
    state.reader.viewer.fit.set(startup_fit);
    // Heights belong to the document that was just closed; leaving them would
    // have the zoom coordinator anchor against a stale column on the first
    // gesture. ReaderPage re-seeds them from the intrinsic page sizes at the
    // current scale.
    state.reader.document.metrics.css_heights.set(Vec::new());
    // Mirror `FitDims::of` (zoom/target.rs) so the seed scale carries the same
    // geometry as the first live fit: the reader margin comes off the width,
    // the toolbar band comes off the height ONLY for the vertical strip that
    // scrolls under it, and a spread doubles the page width. Crucially, the
    // toolbar band is never subtracted from the width — that phantom 48px of
    // side air is exactly what made a margin-0 vertical fit sit 24px off each
    // edge on the very first frame.
    let mode = state.reader.viewer.mode.get_untracked();
    let margin = state.reader.viewer.page_margin.get_untracked();
    let horizontal = mode == ViewMode::ScrollHorizontal;
    let spread = matches!(mode, ViewMode::Spread);
    let (cw, ch) = state.reader.viewer.container_size.get();
    // The horizontal strip ignores the reader's margin (edge-to-edge
    // carousel), so its usable width is the full container width.
    let side_margin = if horizontal { 0.0 } else { margin };
    let cw_eff = (cw - 2.0 * side_margin).max(1.0);
    let ch_eff = if mode.is_paginated() || horizontal {
        ch.max(1.0)
    } else {
        (ch - TOOLBAR_H).max(1.0)
    };
    let pw = if spread { page1.width * 2.0 } else { page1.width };
    // Horizontal Fit Page is a pure height fit (full page visible), matching
    // `FitDims::fit`; every other mode is the shared edge-to-edge `fit_scale`
    // with no extra padding.
    let scale = if horizontal {
        match startup_fit {
            FitMode::Width => clamp_scale(cw_eff / pw.max(1.0)),
            FitMode::Page => clamp_scale(ch_eff / page1.height.max(1.0)),
            FitMode::None => clamp_scale(1.0),
        }
    } else {
        fit_scale(startup_fit, cw_eff, ch_eff, pw, page1.height, 0.0, 1.0)
    };
    // Seeding the zoom state is correct HERE and nowhere else: this is the
    // initial scale for a brand-new document, so there is no layout to
    // animate from and nothing to anchor to. All three scales start in
    // agreement, with no transition in flight.
    state.reader.viewer.zoom.initialize(scale);

    Seeded {
        name,
        resume,
        num_pages,
    }
}

/// Intrinsic (scale-1) size of every page, packed one `PageSize` each.
///
/// The engine sends the widths and heights as two parallel arrays; a book
/// whose arrays do not both match the page count is not trustworthy per-page,
/// so every page falls back to page 1's size rather than being read off by
/// one.
fn intrinsic_sizes(
    widths: &[f64],
    heights: &[f64],
    page1: &PageSize,
    num_pages: u32,
) -> Vec<PageSize> {
    let n = num_pages as usize;
    if widths.len() == n && heights.len() == n {
        widths
            .iter()
            .zip(heights.iter())
            .map(|(&width, &height)| PageSize { width, height })
            .collect()
    } else {
        vec![page1.clone(); n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: f64, h: f64) -> PageSize {
        PageSize {
            width: w,
            height: h,
        }
    }

    #[test]
    fn per_page_sizes_are_used_when_both_arrays_match_the_book() {
        let sizes = intrinsic_sizes(&[10.0, 20.0], &[100.0, 200.0], &size(1.0, 1.0), 2);
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes[1].width, 20.0);
        assert_eq!(sizes[1].height, 200.0);
    }

    #[test]
    fn a_mismatched_array_falls_back_to_page_one_for_every_page() {
        // Reading a short array off by one would give later pages the wrong
        // geometry, which the virtualizer would then lay out against.
        let sizes = intrinsic_sizes(&[10.0], &[100.0, 200.0], &size(612.0, 792.0), 2);
        assert_eq!(sizes.len(), 2);
        assert!(sizes.iter().all(|s| s.width == 612.0 && s.height == 792.0));
    }

    #[test]
    fn a_book_with_no_pages_has_no_sizes() {
        assert!(intrinsic_sizes(&[], &[], &size(612.0, 792.0), 0).is_empty());
    }
}
