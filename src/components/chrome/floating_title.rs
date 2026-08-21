//! Difference-blend floating document name, parked TOP-LEFT.
//!
//! Overlap policy: the label may sit over the page canvas, but may cover at
//! most `MAX_CANVAS_OVERLAP` of the canvas width. Its budget is the blank gap
//! left of the page plus that overlap allowance (minus a safety margin); the
//! name shows in full only when its NATURAL width fits that budget, otherwise
//! it disappears entirely — it never truncates over the page.
//!
//! Shown only when a document is open, the sidebar is OFF (its identity row
//! already shows the name) AND the titlebar is not visible (the bar contains
//! the name). Blend note: `mix-blend-difference` must reach the page pixels,
//! so the wrapper carries NO z-index — a positioned wrapper with z-index forms
//! a stacking context that isolates the blend (the old centered label read as
//! plain white).

use leptos::html;
use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use crate::state::SidebarMode;
use crate::state::AppState;
use super::title_bar::TitleBarCtx;

/// Fraction of the canvas width the label may cover.
const MAX_CANVAS_OVERLAP: f64 = 0.25;
/// Safety margin subtracted from the budget (right inset + breather).
const SAFETY: f64 = 8.0;
/// Minimum budget to even attempt showing the label.
const MIN_LABEL_W: f64 = 40.0;

#[component]
pub fn FloatingDocumentTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let label_ref: NodeRef<html::Span> = NodeRef::new();
    // Allowed total width in px, or None = hide.
    let budget = RwSignal::new(None::<f64>);
    // Natural (unclipped) width of the label; Infinity until first measured.
    let label_w = RwSignal::new(f64::INFINITY);

    let measure = move || {
        request_animation_frame(move || {
            // Mid-zoom relayout: geometry is moving; the effect re-runs when
            // zoom_animating drops, so skipping here loses nothing.
            if state.reader.viewer.zoom_animating.get_untracked() { return; }

            // THE page under the eyes, by id — never an arbitrary mounted page.
            let page = state.reader.viewer.page.get_untracked().max(1);
            let host_id = if state.reader.viewer.mode.get_untracked() == pdf_core::layout::ViewMode::Single {
                format!("sp-{page}-pg")
            } else {
                format!("cont-{}-pg", page.saturating_sub(1))
            };
            let Some(doc_el) = crate::components::dom::by_id(&host_id) else { return };
            let Some(viewer) = crate::components::dom::by_id("viewer-slot") else { return };

            let pr = doc_el.get_bounding_client_rect();
            let vr = viewer.get_bounding_client_rect();
            let canvas_w = pr.width();
            if canvas_w <= 0.0 { return; } // not laid out yet: keep last budget

            let gap = (pr.left() - vr.left()).max(0.0);
            let new_budget = gap + MAX_CANVAS_OVERLAP * canvas_w - SAFETY;

            // Only write on a real change — avoids class/style closure churn
            // every rAF during the sidebar slide.
            if budget.get_untracked().is_none_or(|b| (b - new_budget).abs() > 0.5) {
                budget.set(Some(new_budget));
            }
            if let Some(span) = label_ref.get() {
                let w = span.scroll_width() as f64;
                if w > 0.0 && (label_w.get_untracked() - w).abs() > 0.5 {
                    label_w.set(w);
                }
            }
        });
    };

    // Re-measure whenever geometry or identity can change, and on resize.
    Effect::new(move |_| {
        _ = state.reader.viewer.container_size.get();
        _ = state.reader.viewer.page.get();
        _ = state.reader.viewer.mode.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        measure();
        let h = window_event_listener_untyped("resize", move |_| measure());
        on_cleanup(move || h.remove());
    });

    let shown = move || {
        state.reader.document.status.get() == DocStatus::Ready
            && state.ui.sidebar.get() == SidebarMode::None
            && ctx.map(|c| !c.visible.get()).unwrap_or(true)
            && budget.get().is_none_or(|b| label_w.get() <= b)  // None = unknown = show
    };

    view! {
        // NO z-index on the wrapper: mix-blend-difference must reach page
        // pixels. opacity-0 (not `hidden`) keeps the span measurable.
        <div class="pointer-events-none absolute left-3 top-3">
            <span
                node_ref=label_ref
                class="block truncate text-sm font-medium text-white mix-blend-difference transition-opacity duration-200"
                class=("opacity-0", move || !shown())
                style:max-width=move || match budget.get() {
                    Some(b) if b >= MIN_LABEL_W => format!("{}px", b.max(0.0).floor()),
                    Some(_) => "0px".to_string(),
                    None => "none".to_string(),
                }
            >
                {move || pdf_core::filename::display_name(
                    state.reader.document.title.get().as_deref(),
                    state.reader.document.path.get().as_deref(),
                )
                .unwrap_or_default()}
            </span>
        </div>
    }
}
// The toolbar's document-name label: correct name, and truncation only when
// the name would actually collide with something.
//
// ## What was wrong
//
// 1. The label rendered `doc.title.or(doc.path)` verbatim, so a PDF whose
//    `/Title` metadata is a stale producer path ("file:///F|/Mis%20docum")
//    showed that path instead of the file.s name. `pdf_core::filename` owns the
//    rules that pick a trustworthy name; this component just renders it.
// 2. It carried a hard `max-w-40` (160px), so names were folded with `…` while
//    the toolbar still had plenty of free space.
//
// ## How the truncation works now
//
// The label gets a *measured* `max-width` in px equal to the real gap between
// the buttons on its left and the nearest thing on its right, so `truncate`
// only ever engages on an actual collision:
//
// ```text
//  ┌ px-3 ┬── #toolbar-left-pre ──┬── this label ──┬ … ┬ #toolbar-center ┬ … ┬ #toolbar-right ─┬ px-3 ┐
//         │ hamburger + Open      │ max-width  ->  │   │ page nav        │   │ search/zoom/…   │
// ```
//
// * When a document is open the page nav is absolutely centered in the
//   viewport (Single and Continuous), so the label may only grow until it
//   reaches the nav's left edge.
// * With no document open there is no centered nav, so the label may grow
//   all the way to the right-hand control group.
// * Either way the right group is a hard stop, and everything is derived from
//   the live element WIDTHS, so a window resize re-measures automatically.
//
// Measuring widths (never positions) is deliberate: the label sits between the
// measured elements, so if the maths depended on its neighbours' x-positions,
// a wide label would push them, shrink the computed budget, shrink the label,
// un-push them... a feedback loop that oscillates. Widths of the left/right
// groups are independent of the label, so the computation is stable in one
// pass.
//
// Below `TITLE_MIN_LABEL_W` there is no useful name left to show ("P…"), so the
// label is hidden entirely rather than rendered as a stub — that is the
// window-width awareness on very narrow windows.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use pdf_core::filename::display_name;

/// Left padding of the toolbar row: room for the native traffic lights when
/// the sidebar is closed (see `components/metrics`).
const ROW_PAD_LEFT: f64 = crate::components::metrics::TRAFFIC_LIGHT_INSET;
/// Space on the right the measured name must not enter (see
/// `components/metrics::PIN_RESERVE`; the pin lives OUTSIDE `#toolbar-right`).
const ROW_PAD_RIGHT: f64 = crate::components::metrics::PIN_RESERVE;
/// Gap between the label and the buttons to its left (`gap-1` in the left group).
const GAP_LEFT: f64 = 4.0;
/// Gap the label keeps from whatever is on its right (the centered page nav or
/// the right-hand control group). A little wider than the row's `gap-2` so the
/// name never appears to touch the next control.
const GAP_RIGHT: f64 = 12.0;
/// Narrower than this and the label would be a useless stub — hide it instead.
const TITLE_MIN_LABEL_W: f64 = crate::components::metrics::MIN_DOC_TITLE_WIDTH;

/// Live width of an element by id, or `None` when it isn't in the DOM.
fn width_of(id: &str) -> Option<f64> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .map(|el| el.get_bounding_client_rect().width())
}

/// Compute the label's available width in CSS px from the toolbar's live
/// geometry. `None` when the toolbar isn't laid out yet (measure again later).
fn measure_available(sidebar_open: bool) -> Option<f64> {
    let row_w = width_of("toolbar-row")?;
    if row_w <= 0.0 {
        return None;
    }
    // Buttons to the LEFT of the label (hamburger + Open).
    let pre_w = width_of("toolbar-left-pre").unwrap_or(0.0);
    // The right-hand control group is always a hard stop.
    let right_w = width_of("toolbar-right").unwrap_or(0.0);

    let pad_left = if sidebar_open { 12.0 } else { ROW_PAD_LEFT };
    let start = pad_left + pre_w + GAP_LEFT;
    let mut end = row_w - ROW_PAD_RIGHT - right_w - GAP_RIGHT;

    // When a document is Ready the page nav is absolutely centered on the ROW,
    // so its left edge is (row/2 - nav/2) regardless of the flex groups around
    // it. Absent on the library/empty screen, where the label may run to the
    // right group.
    if let Some(center_w) = width_of("toolbar-center")
        && center_w > 0.0
    {
        end = end.min(row_w / 2.0 - center_w / 2.0 - GAP_RIGHT);
    }

    Some((end - start).max(0.0))
}

#[component]
pub fn DocumentTitle(state: AppState) -> impl IntoView {
    // Measured budget. `None` = not measured yet: render unconstrained for that
    // first frame rather than guessing a width (a wrong guess would visibly
    // fold a name that fits).
    let avail = RwSignal::new(None::<f64>);

    // Keep the observer + its closure alive for the component's lifetime.
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    // Re-measure after the browser has laid out the current frame. Called from
    // both the ResizeObserver and the reactive triggers below.
    let remeasure = move || {
        request_animation_frame(move || {
            let sidebar_open = state.ui.sidebar.get_untracked() != SidebarMode::None;
            if let Some(w) = measure_available(sidebar_open) {
                // Only write on a real change: an idempotent write would still
                // notify and re-run the class/style closures every frame the
                // observer fires during the sidebar's 300ms width animation.
                let prev = avail.get_untracked();
                if prev.is_none_or(|p: f64| (p - w).abs() > 0.5) {
                    avail.set(Some(w));
                }
            }
        });
    };

    // Observe the row and both control groups. The row covers window resizes
    // and the sidebar slide; the groups cover their own content changing
    // (e.g. the zoom readout going from "100%" to "137%", which really does
    // steal space from the name).
    Effect::new(move |_| {
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |_: Vec<ResizeObserverEntry>| {
                remeasure();
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) else {
            return;
        };
        let doc = web_sys::window().and_then(|w| w.document());
        let mut observed = false;
        if let Some(doc) = doc {
            for id in ["toolbar-row", "toolbar-left-pre", "toolbar-right"] {
                if let Some(el) = doc.get_element_by_id(id) {
                    observer.observe(&el);
                    observed = true;
                }
            }
        }
        if observed {
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        } else {
            // Nothing to watch (toolbar not in the DOM yet): drop the observer
            // rather than leaking it, and let the next effect run retry.
            observer.disconnect();
        }
    });

    // Disconnect BEFORE the Closure is dropped. The browser holds its own
    // reference to the wasm-bindgen shim, so a resize notification queued while
    // this component tears down would call into freed memory and abort the
    // runtime ("closure invoked recursively or after being dropped").
    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    // Reactive re-measure triggers. The centered page nav MOUNTS and UNMOUNTS
    // with document Ready (it is not observable while absent), and its width
    // grows with the page count's digits ("/ 9" vs "/ 1024"); a new document
    // changes both. The name itself is included so the first real name measures
    // against the settled toolbar.
    let status = state.reader.document.status;
    let num_pages = state.reader.document.num_pages;
    let title = state.reader.document.title;
    let path = state.reader.document.path;
    let sidebar = state.ui.sidebar;
    Effect::new(move |_| {
        _ = status.get();
        _ = num_pages.get();
        _ = title.get();
        _ = path.get();
        _ = sidebar.get();
        remeasure();
    });

    // The displayed name: a trustworthy `/Title`, else the file name from the
    // path (see pdf_core::filename for why the title cannot simply be believed).
    let name = move || {
        display_name(title.get().as_deref(), path.get().as_deref())
            .unwrap_or_else(|| "No document".to_string())
    };

    // Full name in the tooltip whenever it is (or could be) folded, so a
    // truncated name is always recoverable.
    let full = move || name();

    let hidden = move || avail.get().is_some_and(|w| w < TITLE_MIN_LABEL_W);

    view! {
        <span
            id="toolbar-doc-title"
            data-tauri-drag-region="true"
            class="min-w-0 shrink truncate text-sm text-ink"
            class=("hidden", hidden)
            title=full
            // `max-width` only ever CONSTRAINS: a name shorter than the budget
            // keeps its natural width and never shows an ellipsis. Before the
            // first measurement no cap is applied at all, so the very first
            // paint can't fold a name that fits.
            style:max-width=move || match avail.get() {
                Some(w) if w >= TITLE_MIN_LABEL_W => format!("{}px", w.floor()),
                Some(_) => "0px".to_string(),
                None => "none".to_string(),
            }
        >
            {name}
        </span>
    }
}


