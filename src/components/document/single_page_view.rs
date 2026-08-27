//! Single-page (page-at-a-time) view.
//! One centered PageCanvas in a scroll container. Container size is tracked via
//! ResizeObserver; fit modes (Width/Page) are recomputed reactively.
//!
//! `page` is remounted per page by a keyed `<For>` (`PageCanvas` keys
//! on ids, not the page number — a fresh wrapper + host is mounted per page).
//! Each remount gets a direction-aware entrance animation (`page-enter-right`
//! for next page, `page-enter-left` for previous) driven by a non-reactive
//! `StoredValue` of the previously shown page.
//!
//! The `<For>` keying is load-bearing: a plain reactive block would patch the
//! wrapper div in place (tachys `value.rebuild`), so the CSS animation would not
//! restart on consecutive same-direction turns. Only a keyed remount inserts a
//! fresh node for the animation to replay on — and the unchanged key on a
//! same-page set leaves the existing node untouched (no spurious animation).

use leptos::prelude::*;

use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::document::paginated::PaginatedShell;
use crate::components::document::PageCanvas;
use crate::components::primitives::hooks::dom::SINGLE_PAGE_CONTAINER_ID;
use crate::state::ReaderState;
use crate::state::TextureSignal;

#[component]
pub fn SinglePageView(state: ReaderState) -> impl IntoView {
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let display_scale = state.viewer.zoom.display.read_only();
    let prev_page = StoredValue::new(0u32);

    view! {
        <PaginatedShell state=state scroller_id=SINGLE_PAGE_CONTAINER_ID>
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
        </PaginatedShell>
    }
}
