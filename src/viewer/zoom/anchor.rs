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

use pdf_core::layout::{ViewMode, TOOLBAR_H};

use leptos::prelude::*;

use crate::components::primitives::hooks::dom::{h_page_list, page_list};
use crate::state::reader::{ReaderState, ZoomFocus};
use crate::viewer::engine::ViewerEngine;

/// The centre of page `index`, in the strip's CONTENT coordinates — the
/// point the stage transform pivots on.
///
/// Vertical: y is the middle of the page's extent (virtualizer offsets),
/// x is the strip's horizontal centre (pages are centred, so the page
/// centre is the scroller's mid-width).
/// Horizontal: x is the middle of the page's main-axis extent, y is the
/// vertical centre of the strip (every page is centred by
/// `align-items: center`, so the strip's mid-height is each page's centre
/// whether the strip overflows or not).
pub(crate) fn page_center_origin(
    engine: &ViewerEngine,
    mode: ViewMode,
    index: usize,
    count: usize,
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
            let x = page_list()
                .map(|el| el.client_width() as f64 * 0.5)
                .unwrap_or(0.0);
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
            let y = h_page_list()
                .map(|el| el.scroll_height() as f64 * 0.5)
                .unwrap_or(0.0);
            (x, y)
        }
        // Paginated modes have no strip scroll; the shell's stage pivots on
        // the viewport centre and needs no coordinates.
        _ => (0.0, 0.0),
    }
}

/// The stage pivot for a transaction: the centre of the page the reader is
/// on.
pub(crate) fn stage_origin(engine: &ViewerEngine, state: &ReaderState, mode: ViewMode) -> (f64, f64) {
    let page = state.viewer.page.get_untracked().max(1);
    let index = (page - 1) as usize;
    let count = state.document.num_pages.get_untracked() as usize;
    page_center_origin(engine, mode, index, count)
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

    let (origin_x, origin_y) = page_center_origin(engine, mode, index, count);

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
