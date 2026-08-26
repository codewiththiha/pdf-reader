//! Horizontal strip view: all pages in one virtualized horizontal scrollport.

use leptos::html;
use leptos::prelude::*;
use pdf_core::layout::TOOLBAR_H;
use virtual_list_leptos::{VirtualItem, Virtualizer};
use wasm_bindgen::JsCast;

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::primitives::hooks::dom::H_PAGE_LIST_ID;
use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn HorizontalView(state: ReaderState, virtualizer: Virtualizer) -> impl IntoView {
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let v = virtualizer;
    let list_ref: NodeRef<html::Div> = NodeRef::new();
    {
        let v = v.clone();
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            let el: web_sys::Element = div.clone().unchecked_into();
            v.bind_container(el);
            v.remeasure_container();
            let page = state.viewer.page.get_untracked();
            if page > 0 {
                use virtual_list_leptos::{Align, ScrollMode};
                v.scroll_to_index((page - 1) as usize, Align::Start, ScrollMode::Instant);
            }
        });
    }
    observe_content_size(H_PAGE_LIST_ID, state.viewer.container_size);
    let display_scale = state.viewer.zoom.display.read_only();
    let handle = StoredValue::new_local(v.clone());
    let items = v.items();
    let total_size = v.total_size();
    view! {
        <div
            id=H_PAGE_LIST_ID
            node_ref=list_ref
            class="h-full w-full overflow-x-auto overflow-y-hidden outline-none"
            tabindex="0"
        >
            <div class="relative h-full" style:width=move || format!("{}px", total_size.get())>
                <For
                    each=move || items.get()
                    key=|item: &VirtualItem| item.index
                    children=move |item: VirtualItem| {
                        let index = item.index;
                        let page = (index + 1) as u32;
                        let left = handle.with_value(|v| v.item_top(index));
                        let style = move || format!(
                            "position:absolute;top:{}px;left:{}px;height:100%;display:flex;align-items:flex-start",
                            TOOLBAR_H, left.get()
                        );
                        let geo = Callback::new(move |(page, w, _h): (u32, f64, f64)| {
                            if w > 0.0 {
                                handle.with_value(|v| v.report_size(index, w));
                            }
                            let _ = page;
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
    }
}
