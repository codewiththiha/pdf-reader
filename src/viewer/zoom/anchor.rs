//! The zoom focus and the stage origin: the two things a zoom transaction
//! has to know about position — both PAGE-CENTRIC.
//!
//! Keeping the viewport centre fixed (a proportional anchor) lets the page
//! being read drift across the screen during a zoom and makes the settle
//! jump the reader forward or back whenever the page was not perfectly
//! centred. Instead, a transaction pins the ACTIVE PAGE's centre: it
//! captures where that centre sits on screen (viewport pixels), pivots the
//! stage transform on the page centre (content coordinates), and at commit
//! restores the scroll so the page centre lands on the exact same screen
//! pixel it started on. The page under the reader's eyes never moves; the
//! rest of the surface scales around it.
//!
//! The focus is built from `viewer.page` — never the virtualizer's
//! dominant item, the value most likely to move while a transaction is in
//! flight. There is exactly ONE focus per transaction.
//!
//! Cross-axis centres are computed MATHEMATICALLY (intrinsic sizes ×
//! scale), never from DOM `scrollWidth`/`scrollHeight`: at commit time the
//! DOM has not re-laid out yet, so those queries answer the PRE-scale
//! geometry and the restore would park the page on its old centre instead
//! of tracking the zoom.

use pdf_core::layout::{ViewMode, TOOLBAR_H};

use leptos::prelude::*;

use crate::components::primitives::hooks::dom::{h_page_list, page_list};
use crate::state::reader::{ReaderState, ZoomFocus};
use crate::viewer::engine::ViewerEngine;

/// The centre of page `index`, in the strip's CONTENT coordinates — the
/// point the stage transform pivots on — at `scale`.
///
/// Vertical: y is the middle of the page's extent (virtualizer offsets),
/// x is the centre of the content width — the widest page at `scale` plus
/// the two horizontal margins, or the viewport when the content fits.
/// Horizontal: x is the middle of the page's main-axis extent, y is the
/// centre of the strip's height — the tallest page at `scale`, or the
/// viewport when the strip fits. Both cross-axis values come from the
/// intrinsic sizes and the scale, so the answer is correct even in the
/// same tick as the commit's rescale (the DOM would still be reporting
/// the old extent).
pub(crate) fn page_center_origin(
    engine: &ViewerEngine,
    state: &ReaderState,
    mode: ViewMode,
    index: usize,
    count: usize,
    scale: f64,
) -> (f64, f64) {
    match mode {
        ViewMode::ScrollVertical => {
            let start = engine.vertical.offset_of(index);
            let end = if index + 1 < count {
                engine.vertical.offset_of(index + 1)
            } else {
                engine.vertical.total_size().get_untracked()
            };
            let y = (start + end) * 0.5;

            let client_w = page_list()
                .map(|el| el.client_width() as f64)
                .unwrap_or(0.0);
            let max_w = state.document.metrics.intrinsic.with_untracked(|sizes| {
                sizes.iter().map(|s| s.width).fold(0.0, f64::max)
            });
            let margin = state.viewer.page_margin.get_untracked();
            let content_w = (max_w * scale + margin * 2.0).max(client_w);
            let x = content_w * 0.5;
            (x, y)
        }
        ViewMode::ScrollHorizontal => {
            let start = engine.horizontal.offset_of(index);
            let end = if index + 1 < count {
                engine.horizontal.offset_of(index + 1)
            } else {
                engine.horizontal.total_size().get_untracked()
            };
            let x = (start + end) * 0.5;

            let client_h = h_page_list()
                .map(|el| el.client_height() as f64)
                .unwrap_or(0.0);
            let max_h = state.document.metrics.intrinsic.with_untracked(|sizes| {
                sizes.iter().map(|s| s.height).fold(0.0, f64::max)
            });
            let content_h = (max_h * scale).max(client_h);
            let y = content_h * 0.5;
            (x, y)
        }
        // Paginated modes have no strip scroll; the shell's stage pivots on
        // the viewport centre and needs no coordinates.
        _ => (0.0, 0.0),
    }
}

/// The stage pivot for a transaction: the centre of the page the reader is
/// on, at the committed scale.
pub(crate) fn stage_origin(engine: &ViewerEngine, state: &ReaderState, mode: ViewMode) -> (f64, f64) {
    let page = state.viewer.page.get_untracked().max(1);
    let index = (page - 1) as usize;
    let count = state.document.num_pages.get_untracked() as usize;
    let scale = state.viewer.zoom.committed.get_untracked();
    page_center_origin(engine, state, mode, index, count, scale)
}

/// Capture where the reader's eyes are, immediately before a transaction
/// opens: the active page and the viewport pixels its centre currently
/// sits at. The commit restores exactly those offsets, so the page centre
/// stays on the same screen pixel across the whole zoom.
pub(crate) fn capture_focus(engine: &ViewerEngine, state: &ReaderState) -> ZoomFocus {
    let page = state.viewer.page.get_untracked().max(1);
    let index = (page - 1) as usize;
    let count = state.document.num_pages.get_untracked() as usize;
    let mode = state.viewer.mode.get_untracked();
    let scale = state.viewer.zoom.committed.get_untracked();

    let (origin_x, origin_y) = page_center_origin(engine, state, mode, index, count, scale);

    let (viewport_offset_x, viewport_offset_y) = match mode {
        // The strip's content starts TOOLBAR_H below the scroller's origin
        // (the pages scroll under a fixed toolbar), so the page centre's
        // on-screen y is its content y plus that band, minus the scroll.
        ViewMode::ScrollVertical => page_list()
            .map(|el| {
                let scroll_top = el.scroll_top() as f64;
                let scroll_left = el.scroll_left() as f64;
                (origin_x - scroll_left, (origin_y + TOOLBAR_H) - scroll_top)
            })
            .unwrap_or((0.0, 0.0)),
        ViewMode::ScrollHorizontal => h_page_list()
            .map(|el| {
                let scroll_top = el.scroll_top() as f64;
                let scroll_left = el.scroll_left() as f64;
                (origin_x - scroll_left, origin_y - scroll_top)
            })
            .unwrap_or((0.0, 0.0)),
        _ => (0.0, 0.0),
    };

    ZoomFocus {
        page,
        viewport_offset_x,
        viewport_offset_y,
    }
}
