//! Dual-page (spread) view: two pages side by side with no gap.

use leptos::prelude::*;

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::primitives::hooks::dom::DUAL_PAGE_CONTAINER_ID;
use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn DualPageView(state: ReaderState) -> impl IntoView {
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let display_scale = state.viewer.zoom.display.read_only();
    let prev_spread = StoredValue::new(0u32);
    observe_content_size(DUAL_PAGE_CONTAINER_ID, state.viewer.container_size);
    view! {
        <div
            id=DUAL_PAGE_CONTAINER_ID
            class="flex h-full w-full items-start justify-center overflow-auto bg-surface"
        >
            <div class="px-6 pb-6 pt-18">
                // Keyed remount per spread so the page-turn animation replays.
                <For
                    each=move || std::iter::once({
                        let p = state.viewer.page.get().max(1);
                        (p - 1) / 2
                    })
                    key=|s: &u32| *s
                    children=move |spread: u32| {
                        let n = state.document.num_pages.get();
                        let p1 = spread * 2 + 1;
                        let p2 = p1 + 1;
                        let prev = prev_spread.get_value();
                        let dir = if prev == 0 || spread > prev {
                            "page-enter-right"
                        } else {
                            "page-enter-left"
                        };
                        prev_spread.set_value(spread);
                        view! {
                            <div class=dir>
                                // gap-0: the two pages touch, like an open book.
                                <div class="flex items-start justify-center gap-0">
                                    <PageCanvas
                                        page=p1
                                        scale=display_scale
                                        render_scale=state.viewer.zoom.render
                                        zoom_animating=state.viewer.zoom_animating
                                        texture=texture
                                        canvas_id=format!("dp-{p1}-cv")
                                        host_id=format!("dp-{p1}-pg")
                                        render_text=true
                                        gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                    />
                                    {if p2 <= n {
                                        view! {
                                            <PageCanvas
                                                page=p2
                                                scale=display_scale
                                                render_scale=state.viewer.zoom.render
                                                zoom_animating=state.viewer.zoom_animating
                                                texture=texture
                                                canvas_id=format!("dp-{p2}-cv")
                                                host_id=format!("dp-{p2}-pg")
                                                render_text=true
                                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                            />
                                        }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                </div>
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
