//! Difference-blend floating document name. Shown ONLY when a document is
//! open, the sidebar is OFF (its identity row already shows the name
//! otherwise) AND the titlebar is NOT visible (the bar already contains the
//! name).

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use pdf_viewer::state::SidebarMode;
use crate::core::state::AppState;
use super::titlebar_provider::TitleBarCtx;

#[component]
pub fn FloatingDocTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let hidden = move || {
        state.doc.status.get() != DocStatus::Ready
            || state.sidebar.get() != SidebarMode::None
            || ctx.map(|c| c.visible.get()).unwrap_or(false)
    };
    view! {
        <div
            class="pointer-events-none absolute inset-x-0 top-3 z-30 flex justify-center transition-opacity duration-200"
            class=("opacity-0", hidden)
        >
            <span class="max-w-[60%] truncate text-sm text-white mix-blend-difference">
                {move || pdf_core::filename::display_name(
                    state.doc.title.get().as_deref(),
                    state.doc.path.get().as_deref(),
                )
                .unwrap_or_default()}
            </span>
        </div>
    }
}
