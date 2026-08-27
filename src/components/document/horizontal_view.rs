//! Horizontal strip view: all pages in one virtualized horizontal scrollport.
//!
//! WHEEL POLICY. The strip is a horizontal scrollport in a world where the
//! wheel is vertical, so exactly one case needs help from us: a plain
//! vertical tick while the strip fits the slot vertically, where the browser
//! has nothing to pan and would do nothing at all. That tick is translated
//! into `scrollLeft`. Every other input is left to the native scroll chain —
//! shift+wheel already maps to horizontal, a trackpad's `deltaX` already
//! scrolls horizontally, and once a zoom makes the strip taller than the slot
//! the plain wheel pans vertically for free.
//!
//! The strip's height is derived from the tallest intrinsic page at the live
//! scale, so that vertical range appears exactly when the zoom exceeds
//! fit-height. That single geometric fact is also what the wheel handler
//! tests, so the two behaviours cannot drift apart into a second state flag.

use leptos::html;
use leptos::prelude::*;
use virtual_list_leptos::{VirtualItem, Virtualizer};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::primitives::hooks::dom::H_PAGE_LIST_ID;
use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::components::viewer_controls::overlay_scrollbar::OverlayScrollbar;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn HorizontalView(state: ReaderState, virtualizer: Virtualizer) -> impl IntoView {
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let v = virtualizer;
    let list_ref: NodeRef<html::Div> = NodeRef::new();

    // Tallest page at the live scale. The strip is at least this tall, so
    // zooming past fit-height turns into REAL vertical scroll range — which
    // is what flips the wheel handler into leaving vertical pans alone.
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.display.get();
        let tallest = state
            .document
            .metrics
            .intrinsic
            .with(|pages| pages.iter().map(|p| p.height).fold(0.0, f64::max));
        tallest * scale
    });

    {
        let v = v.clone();
        // The listener is retained by JS (the closure is leaked into a
        // Function), so the element it is attached to must be remembered in
        // order to detach it again on re-bind or unmount.
        let wheel_guard = StoredValue::new_local(None::<(web_sys::Element, js_sys::Function)>);
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            let el: web_sys::Element = div.clone().unchecked_into();
            v.bind_container(el.clone());
            v.remeasure_container();

            if let Some((old_el, old_fn)) = wheel_guard.get_value() {
                let _ = old_el.remove_event_listener_with_callback("wheel", &old_fn);
            }
            let target = el.clone();
            let cb = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(
                move |e: web_sys::WheelEvent| {
                    if e.shift_key() {
                        // The browser already maps shift+wheel to horizontal.
                        return;
                    }
                    let dx = e.delta_x();
                    let mut dy = e.delta_y();
                    match e.delta_mode() {
                        1 => dy *= 16.0,  // lines (Firefox)
                        2 => dy *= 120.0, // pages
                        _ => {}
                    }
                    if dx.abs() > dy.abs() {
                        // A genuine horizontal gesture: the native chain has it.
                        return;
                    }
                    if target.scroll_height() - target.client_height() > 1 {
                        // Zoomed in past fit-height: let the vertical pan happen.
                        return;
                    }
                    // The strip fits vertically, so a vertical tick would do
                    // nothing. Drive the strip with it instead.
                    e.prevent_default();
                    target.set_scroll_left((target.scroll_left() as f64 + dy) as i32);
                },
            );
            let handler: js_sys::Function = cb.into_js_value().unchecked_into();
            // Non-passive: the fits-vertically branch calls preventDefault.
            let opts = web_sys::AddEventListenerOptions::new();
            opts.set_passive(false);
            let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
                "wheel",
                &handler,
                &opts,
            );
            wheel_guard.set_value(Some((el, handler)));
            on_cleanup(move || {
                if let Some((old_el, old_fn)) = wheel_guard.get_value() {
                    let _ = old_el.remove_event_listener_with_callback("wheel", &old_fn);
                }
                wheel_guard.set_value(None);
            });

            let page = state.viewer.page.get_untracked();
            if page > 0 {
                use virtual_list_leptos::{Align, ScrollMode};
                v.scroll_to_index((page - 1) as usize, Align::Center, ScrollMode::Instant);
            }
        });
    }
    observe_content_size(H_PAGE_LIST_ID, state.viewer.container_size);
    let display_scale = state.viewer.zoom.display.read_only();
    let handle = StoredValue::new_local(v.clone());
    let items = v.items();
    let total_size = v.total_size();
    view! {
        <div class="relative h-full w-full">
            <div
                id=H_PAGE_LIST_ID
                node_ref=list_ref
                class="scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
                tabindex="0"
            >
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
                            // the auto-hiding title bar overlays it, like Dual.
                            //
                            // align-items:center — every wrapper is exactly as
                            // tall as the strip and the strip is at least as
                            // tall as the tallest page at this scale, so a page
                            // can never overflow its wrapper and centring can
                            // never clip. A fitted strip then sits in the middle
                            // of the window, and a zoomed one centres each page
                            // against the tallest, like a real book strip.
                            let style = move || format!(
                                "position:absolute;top:0;left:{}px;height:100%;display:flex;align-items:center;padding-inline:{}px",
                                left.get(), state.viewer.page_margin.get()
                            );
                            let geo = Callback::new(move |(_page, w, _h): (u32, f64, f64)| {
                                if w > 0.0 {
                                    let m = state.viewer.page_margin.get_untracked();
                                    handle.with_value(|v| v.report_size(index, w + 2.0 * m));
                                }
                            });
                            view! {
                                <div id=format!("hp-{page}-wrap") style=style>
                                    <PageCanvas
                                        page=page
                                        scale=display_scale
                                        render_scale=state.viewer.zoom.render
                                        zoom_animating=state.viewer.zoom_animating
                                        texture=texture
                                        canvas_id=format!("hp-{page}-cv")
                                        host_id=format!("hp-{page}-pg")
                                        render_text=true
                                        on_geometry=geo
                                        gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                    />
                                </div>
                            }
                        }
                    />
                </div>
            </div>
            <OverlayScrollbar scroller_id=H_PAGE_LIST_ID horizontal=true />
        </div>
    }
}
