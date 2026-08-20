//! Difference-blend floating document name, parked in the TOP-RIGHT corner so
//! it never covers the reader's view.
//!
//! Window- and canvas-aware: it measures the blank space between the right
//! edge of the widest rendered `.pdf-page` and the window edge, and
//!   * hides when the page fills the window (fit-width / fullscreen — no room),
//!   * shows only as many characters as the blank space fits ("barely" when the
//!     gap is narrow, the full name when the page is small / zoomed out), and
//!   * allows a slight overlap of the canvas rather than disappearing outright.
//!
//! Shown ONLY when a document is open, the sidebar is OFF (its identity row
//! already shows the name) AND the titlebar is not visible (the bar contains
//! the name). Blend note: `mix-blend-difference` must reach the page pixels,
//! so the span lives under a wrapper with NO z-index — a positioned wrapper
//! with z-index forms a stacking context that isolates the blend (the old
//! centered label blended only against its own transparent wrapper, which is
//! why it read as plain white). Dropping the z-index lets the span blend
//! against the page canvas beneath it.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_engine::types::DocStatus;
use pdf_viewer::state::SidebarMode;
use crate::core::state::AppState;
use super::titlebar_provider::TitleBarCtx;

/// Below this much usable width the label disappears entirely. Low on purpose:
/// "barely any room" still shows a couple of characters (a slight overlap of
/// the canvas is acceptable, per the reference), and only a truly full-width
/// page (fit-width / fullscreen) has zero room and hides it.
const MIN_BLANK_W: f64 = 36.0;
/// Reserved between the page's right edge and the label: the label's own 12px
/// right inset (right-3) plus a 4px breather from the canvas edge.
const SIDE_MARGIN: f64 = 16.0;
/// Cap so a small page in a huge window does not make a comically wide label.
const MAX_W: f64 = 480.0;

/// Right edge (viewport x) of the widest currently-mounted page, if any.
fn rightmost_page_edge() -> Option<f64> {
    let doc = web_sys::window()?.document()?;
    let nodes = doc.query_selector_all(".pdf-page").ok()?;
    let mut max = f64::NEG_INFINITY;
    for i in 0..nodes.length() {
        let Some(node) = nodes.item(i) else { continue };
        let Some(el) = node.dyn_ref::<web_sys::Element>() else { continue };
        let r = el.get_bounding_client_rect();
        // Skip shells that have not been laid out / sized yet.
        if r.width() > 0.0 {
            max = max.max(r.right());
        }
    }
    max.is_finite().then_some(max)
}

#[component]
pub fn FloatingDocTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    // Available width (px) for the label, or None = hide.
    let avail = RwSignal::new(None::<f64>);

    let measure = move || {
        let Some(win) = web_sys::window() else { return };
        let win_w = win
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        match rightmost_page_edge() {
            None => avail.set(None),
            Some(page_right) => {
                let blank = (win_w - page_right).max(0.0);
                let usable = (blank - SIDE_MARGIN).min(MAX_W);
                if usable >= MIN_BLANK_W {
                    avail.set(Some(usable));
                } else {
                    avail.set(None);
                }
            }
        }
    };

    // Re-measure on every layout-affecting change. The page hosts mount
    // asynchronously after Ready, so measure on the frame after the reactive
    // flush (rAF) and once more on the following frame — pages that mount in
    // between are picked up, and the fit/zoom systems re-run this effect via
    // display_scale / container_size as the layout settles.
    Effect::new(move |_| {
        _ = state.viewer.mode.get();
        _ = state.viewer.display_scale.get();
        _ = state.viewer.render_scale.get();
        _ = state.viewer.container_size.get();
        _ = state.sidebar.get();
        _ = state.doc.status.get();
        _ = state.doc.num_pages.get();
        request_animation_frame(move || {
            measure();
            request_animation_frame(measure);
        });
    });

    // Window resize re-clamps.
    Effect::new(move |_| {
        let h = window_event_listener_untyped("resize", move |_| measure());
        on_cleanup(move || h.remove());
    });

    let hidden = move || {
        state.doc.status.get() != DocStatus::Ready
            || state.sidebar.get() != SidebarMode::None
            || ctx.map(|c| c.visible.get()).unwrap_or(false)
            || avail.get().is_none()
    };

    view! {
        <div class="pointer-events-none absolute top-3 right-3">
            <span
                class="inline-block truncate text-sm font-medium text-white mix-blend-difference"
                class=("opacity-0", hidden)
                style:max-width=move || avail
                    .get()
                    .map(|w| format!("{}px", w.floor()))
                    .unwrap_or_else(|| "0px".to_string())
            >
                {move || pdf_core::filename::display_name(
                    state.doc.title.get().as_deref(),
                    state.doc.path.get().as_deref(),
                )
                .unwrap_or_default()}
            </span>
        </div>
    }
}
