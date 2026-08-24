//! Anchor tracking — the PDF-specific replacement for the Gloss reference's
//! `<mark>`.
//!
//! The reference wraps the selected word in a DOM mark and re-measures it on
//! scroll. Nothing DOM-shaped survives here: pdf.js rebuilds the textLayer
//! `<span>`s on every zoom, the virtualizer unmounts whole pages as they
//! scroll out of the render window, and the native `Selection` we could have
//! leaned on is cleared the moment the card opens (it fights the card's own
//! text selection) — besides being unserializable and singular.
//!
//! So the anchor is not a node at all. A selection is captured ONCE as a
//! **page-space rect** ([`capture_selection_mark`]) — unscaled CSS px from the
//! `.pdf-page` host's origin — and projected back to the screen through
//! whichever host currently owns that page ([`mark_screen_box`]). Remounts,
//! zooms and view-mode flips all just change the projection, exactly like the
//! search highlight layer.

use pdf_core::gloss::{GlossBox, GlossMark};
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::marks::MARK_RADIUS;
use crate::components::document::dom_helpers::by_id;

/// Id of the `.pdf-page` host that currently renders `page`, per view mode.
/// Continuous hosts are keyed 0-based (`cont-{i}-pg`), single-page hosts
/// 1-based (`sp-{page}-pg`) — mirroring `PageList` / `SinglePageView`.
fn host_id_for(page: u32, single: bool) -> String {
    if single {
        format!("sp-{page}-pg")
    } else {
        format!("cont-{}-pg", page.saturating_sub(1))
    }
}

/// Union of the current selection's client rects, converted into page space
/// via the `.pdf-page` host that contains it.
///
/// Must be called BEFORE the selection is cleared. `scale` is the display
/// scale the page is currently drawn at, which is divided out so the stored
/// rect is zoom-independent. `None` when there is no live selection, or when
/// it did not land inside a page host (e.g. chrome text).
pub fn capture_selection_mark(
    page: u32,
    scale: f64,
    word: String,
    context: String,
) -> Option<GlossMark> {
    if scale <= 0.0 {
        return None;
    }
    let sel = web_sys::window()?.get_selection().ok()??;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    let host = el.closest(".pdf-page").ok().flatten()?;
    let hr = host.get_bounding_client_rect();
    // Range::getClientRects is nullable in the IDL, unlike Element's.
    let rects = range.get_client_rects()?;
    let (mut l, mut t, mut r, mut b) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut found = false;
    for i in 0..rects.length() {
        let Some(rc) = rects.get(i) else {
            continue;
        };
        if rc.width() <= 0.0 || rc.height() <= 0.0 {
            continue;
        }
        found = true;
        l = l.min(rc.left());
        t = t.min(rc.top());
        r = r.max(rc.right());
        b = b.max(rc.bottom());
    }
    if !found {
        return None;
    }
    Some(GlossMark {
        // Page + timestamp is unique enough for a per-document list and keeps
        // the id readable in the persisted JSON.
        id: format!("g{page}-{}", js_sys::Date::now() as u64),
        page,
        word,
        context,
        rect: GlossBox {
            x: (l - hr.left()) / scale,
            y: (t - hr.top()) / scale,
            w: ((r - l) / scale).max(1.0),
            h: ((b - t) / scale).max(1.0),
            r: 0.0,
        },
    })
}

/// Screen box for a mark, looked up by host id so it works after remounts.
/// `None` while that page is unmounted — the caller keeps the last box, so the
/// chip freezes instead of jumping to the origin.
///
/// Exact-fit: the stroke is the stored union rect itself (no padding), with
/// a small radius so the morph hand-off lands on the same geometry the
/// highlighter stroke occupies.
pub fn mark_screen_box(mark: &GlossMark, scale: f64, single: bool) -> Option<GlossBox> {
    if scale <= 0.0 {
        return None;
    }
    let hr = by_id(&host_id_for(mark.page, single))?.get_bounding_client_rect();
    let h = mark.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + mark.rect.x * scale,
        y: hr.top() + mark.rect.y * scale,
        w: mark.rect.w * scale,
        h,
        // Exact-fit stroke radius (not a pill capsule).
        r: MARK_RADIUS.min(h / 2.0),
    })
}
