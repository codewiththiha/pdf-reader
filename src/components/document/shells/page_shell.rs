use leptos::children::ChildrenFn;
use leptos::prelude::*;

use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::components::viewer_controls::overlay_scrollbar::OverlayScrollbar;
use crate::state::ReaderState;

/// Shared shell for Single & Spread. The child is centered with `margin:auto`
/// in the true viewport, which degrades to start-alignment on overflow —
/// so both axes scroll exactly to the page edge, never clipped, never with
/// phantom space.
#[component]
pub fn PageShell(
    state: ReaderState,
    scroller_id: &'static str,
    children: ChildrenFn,
) -> impl IntoView {
    observe_content_size(scroller_id, state.viewer.container_size);
    view! {
        <div class="relative h-full w-full">
            <div
                id=scroller_id
                class="paginated-scroller scrollbar-none flex h-full w-full overflow-auto bg-surface"
            >
                <div
                    class="m-auto"
                    style:padding-inline=move || format!("{}px", state.viewer.page_margin.get())
                >
                    {children()}
                </div>
            </div>
            <OverlayScrollbar scroller_id=scroller_id />
        </div>
    }
}
