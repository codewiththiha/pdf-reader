//! Anchor tracking — the PDF-specific replacement for the Gloss reference's
//! `<mark>`.
//!
//! The reference wraps the selected word in a DOM mark and re-measures it on
//! scroll. pdf.js re-renders the textLayer `<span>`s on zoom, so wrapping them
//! is hopeless. Instead we hold a reference to the selected span and
//! re-measure its client rects live. When a zoom re-render detaches it
//! ([`live_anchor_box`] returns `None`), the caller keeps the last box — the
//! chip freezes rather than jumping.

use wasm_bindgen::JsCast;

use pdf_core::gloss::{pad_box, GlossBox};

/// The selected textLayer span(s), captured on the Info click BEFORE the
/// document selection is cleared. `None` if no selection survived.
pub fn capture_selection_anchor() -> Option<web_sys::Element> {
    let sel = web_sys::window()?.get_selection().ok()??;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    // Text node -> its parent span. If the anchor is already an element, use it.
    node.parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())
}

/// Live viewport box of the anchor, padded like the reference (5, 3).
/// Returns `None` when the element has detached (a zoom re-rendered the
/// textLayer) so the caller can keep the last good box.
pub fn live_anchor_box(el: &web_sys::Element) -> Option<GlossBox> {
    // `parent_element` is None once the span was removed from the DOM by a
    // textLayer re-render — the one place the mark-based reference can't be
    // matched exactly.
    if el.parent_element().is_none() {
        return None;
    }
    let rects = el.get_client_rects();
    let (mut l, mut t, mut r, mut b) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut found = false;
    for i in 0..rects.length() {
        let Some(rect) = rects.get(i) else {
            continue;
        };
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        found = true;
        l = l.min(rect.left());
        t = t.min(rect.top());
        r = r.max(rect.right());
        b = b.max(rect.bottom());
    }
    if !found {
        return None;
    }
    let box_ = GlossBox {
        x: l,
        y: t,
        w: (r - l).max(1.0),
        h: (b - t).max(1.0),
        r: 0.0,
    };
    Some(pad_box(box_, 5.0, 3.0))
}
