//! Axis-generic virtualized page strip, shared by the two scrolling layouts.
//!
//! This is the unified replacement for `page_list` (vertical) and the inline
//! loop that used to live inside `HorizontalView`: one component renders the
//! mounted page window along either axis, absolutely positioning each page at
//! the virtualizer's `item_top`, and reporting the rendered main-axis size
//! back into the virtualizer's size model.
//!
//! The strip is pure presentation. It owns no scroll policy, no wheel
//! translation, no container binding — those live in [`ScrollShell`], which
//! creates the scroller element this strip draws into. The page-host ids keep
//! their per-axis prefixes (`cont-` / `hp-`) because the engine's selection
//! and the AI gloss layer parse them back into page numbers.

use leptos::html;
use leptos::prelude::*;
use pdf_core::layout::{Axis, TOOLBAR_H};
use virtual_list_leptos::{VirtualItem, Virtualizer};

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn PageStrip(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    /// The scroller element this strip lays out into (owned by ScrollShell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");

    let v = virtualizer;
    let handle = StoredValue::new_local(v.clone());
    let display_scale = state.viewer.zoom.layout.read_only();
    let items = v.items();
    let total_size = v.total_size();

    // Horizontal-only: the strip is at least as tall as the tallest page at
    // the live layout scale, so a zoom past fit-height yields real vertical
    // scroll range. Only read for the horizontal branch, but cheap either way.
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.layout.get();
        let tallest = state
            .document
            .metrics
            .intrinsic
            .with(|pages| pages.iter().map(|p| p.height).fold(0.0, f64::max));
        tallest * scale
    });

    let scroller_class = match axis {
        Axis::Vertical => "scrollbar-none h-full w-full overflow-y-auto outline-none",
        Axis::Horizontal => {
            "scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
        }
    };

    // Report a rendered page's main-axis extent back into the virtualizer.
    // Vertical uses the measured height (+ gap); horizontal uses the measured
    // width (+ the two horizontal margins, which are part of the main span).
    let on_geometry = match axis {
        Axis::Vertical => {
            let handle = handle.clone();
            Callback::new(move |(page, _w, height): (u32, f64, f64)| {
                if state.viewer.zoom_animating.get_untracked() {
                    return;
                }
                let index = page.saturating_sub(1) as usize;
                state.document.metrics.css_heights.update(|heights| {
                    while heights.len() <= index {
                        heights.push(0.0);
                    }
                    heights[index] = height;
                });
                let gap = state.viewer.page_gap.get_untracked();
                handle.with_value(|v| v.report_size(index, height + gap));
            })
        }
        Axis::Horizontal => {
            let handle = handle.clone();
            Callback::new(move |(_page, w, _h): (u32, f64, f64)| {
                if w > 0.0 {
                    let m = state.viewer.page_margin.get_untracked();
                    handle
                        .with_value(|v| v.report_size(_page.saturating_sub(1) as usize, w + 2.0 * m));
                }
            })
        }
    };

    view! {
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0">
            {match axis {
                Axis::Vertical => {
                    let handle = handle.clone();
                    view! {
                        <div class="relative" style=format!("margin-top:{TOOLBAR_H}px")>
                            <div aria-hidden="true" style:height=move || format!("{}px", total_size.get())></div>
                            <For
                                each=move || items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let top = handle.with_value(|v| v.item_top(index));
                                    let style = move || format!(
                                        "position:absolute;top:{}px;left:0;right:0;display:flex;justify-content:center;padding-inline:{}px",
                                        top.get(), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Vertical, index, page) style=style>
                                            <PageCanvas
                                                page=page
                                                scale=display_scale
                                                render_scale=state.viewer.zoom.render
                                                zoom_animating=state.viewer.zoom_animating
                                                texture=texture
                                                canvas_id=format!("cont-{index}-cv")
                                                host_id=format!("cont-{index}-pg")
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                            />
                                        </div>
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
                Axis::Horizontal => {
                    let handle = handle.clone();
                    view! {
                        <div
                            class="relative"
                            style:width=move || format!("{}px", total_size.get())
                            style:height=move || format!("max(100%, {}px)", strip_h.get().ceil())
                        >
                            <For
                                each=move || items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let left = handle.with_value(|v| v.item_top(index));
                                    // top:0 — the strip owns the full window height and
                                    // the auto-hiding title bar overlays it, like Spread.
                                    let style = move || format!(
                                        "position:absolute;top:0;left:{}px;height:100%;display:flex;align-items:center;padding-inline:{}px",
                                        left.get(), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Horizontal, index, page) style=style>
                                            <PageCanvas
                                                page=page
                                                scale=display_scale
                                                render_scale=state.viewer.zoom.render
                                                zoom_animating=state.viewer.zoom_animating
                                                texture=texture
                                                canvas_id=format!("hp-{page}-cv")
                                                host_id=format!("hp-{page}-pg")
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                            />
                                        </div>
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// Per-axis wrapper id, kept as a free function so both ends of the strip's
/// `<For>` can name it without capturing anything by move.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("cont-{index}-wrap"),
        Axis::Horizontal => format!("hp-{page}-wrap"),
    }
}
