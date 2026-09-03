//! The axis-generic virtualized strip of TEXT pages — the text counterpart
//! of [`PageStrip`](crate::components::document::PageStrip).
//!
//! The shape is the same (mounted window, absolutely-positioned hosts at the
//! virtualizer's offsets, device-pixel snapping), and one difference carries
//! all the others: a text page's size is KNOWN — every page is A4 — so the
//! strip never reports geometry back. There is no `on_geometry`, no render
//! scale to reconcile, no bitmap to stretch: the host IS the content, sized
//! from the live display scale, and the virtualizer's size model is seeded
//! exact by the open flow.
//!
//! Like its PDF twin, the strip is pure presentation: scroll policy and
//! container binding live in the shell that hosts it.

use leptos::html;
use leptos::prelude::*;
use pdf_core::layout::Axis;
use virtual_list_leptos::{VirtualItem, Virtualizer};

use super::page::TextPage;
use crate::components::document::pixel_grid::snap_px;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn TextPageStrip(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    /// The scroller element this strip lays out into (owned by the shell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");

    let v = virtualizer;
    let handle = StoredValue::new_local(v.clone());
    let items = v.items();
    let total_size = v.total_size();

    // Horizontal-only: the strip is at least as tall as the tallest page at
    // the live scale, so a zoom past fit-height yields real vertical scroll
    // range as the zoom happens (the same rule as the PDF strip).
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.display.get();
        text_core::page::PAGE_HEIGHT * scale
    });

    let scroller_class = match axis {
        Axis::Vertical => "scrollbar-none h-full w-full overflow-y-auto outline-none",
        Axis::Horizontal => {
            "scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
        }
    };

    view! {
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0">
            {match axis {
                Axis::Vertical => {
                    let each_items = items;
                    view! {
                        <div class="relative">
                            <div aria-hidden="true" style:height=move || format!("{}px", total_size.get())></div>
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let top = handle.with_value(|v| v.item_top(index));
                                    let style = move || {
                                        format!(
                                            "position:absolute;top:{}px;left:0;right:0;display:flex;padding-inline:{}px",
                                            snap_px(top.get()),
                                            state.viewer.page_margin.get(),
                                        )
                                    };
                                    view! {
                                        <div id=format!("txv-{index}-wrap") style=style>
                                            // The vertical strip renders its
                                            // pages INVISIBLE: no boxes, no
                                            // gap, blocks flowing through —
                                            // the "pages" exist only as the
                                            // virtualizer's unit of work.
                                            <TextPage page=page state=state texture=texture flow=true />
                                        </div>
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
                Axis::Horizontal => {
                    let each_items = items;
                    view! {
                        <div
                            class="relative"
                            style=move || {
                                format!(
                                    "width:{}px;height:max(100%, {}px)",
                                    total_size.get(),
                                    strip_h.get().ceil()
                                )
                            }
                        >
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let left = handle.with_value(|v| v.item_top(index));
                                    let style = move || format!(
                                        "position:absolute;top:0;left:{}px;height:100%;display:flex;padding-inline:{}px",
                                        snap_px(left.get()), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=format!("txh-{page}-wrap") style=style>
                                            <TextPage page=page state=state texture=texture class="my-auto" />
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
