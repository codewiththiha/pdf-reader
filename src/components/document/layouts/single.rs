//! Single-page (`single`) layout: one centered PageCanvas in a scroll container.
//! `page` is remounted per page by a keyed `<For>` so the direction-aware
//! entrance animation replay on each turn (see the comment in the old
//! `SinglePageView` for the load-bearing keying).

use leptos::prelude::*;

use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::document::shells::page_shell::PageShell;
use crate::components::document::PageCanvas;
use crate::components::primitives::hooks::dom::SINGLE_PAGE_CONTAINER_ID;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn SingleLayout(state: ReaderState) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");
    let display_scale = state.viewer.zoom.layout.read_only();
    let gesture_owns = state.viewer.gesture_owns();
    let prev_page = StoredValue::new(0u32);

    view! {
        <PageShell state=state scroller_id=SINGLE_PAGE_CONTAINER_ID>
            <For
                each=move || std::iter::once(state.viewer.page.get())
                key=|p: &u32| *p
                children=move |page: u32| {
                    let prev = prev_page.get_value();
                    let dir = if prev == 0 || page > prev {
                        "page-enter-right"
                    } else {
                        "page-enter-left"
                    };
                    prev_page.set_value(page);
                    view! {
                        <div class=dir>
                            <PageCanvas
                                page=page
                                scale=display_scale
                                render_scale=state.viewer.zoom.render
                                zoom_animating=state.viewer.zoom_animating
                                gesture_owns=gesture_owns
                                texture=texture
                                canvas_id=format!("sp-{page}-cv")
                                host_id=format!("sp-{page}-pg")
                                render_text=true
                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                            />
                        </div>
                    }
                }
            />
        </PageShell>
    }
}
