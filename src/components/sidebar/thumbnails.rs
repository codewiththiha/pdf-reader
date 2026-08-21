//! Thumbnails panel host: the absolutely-stacked panel wrapper with its
//! paint/outro toggles, around the reusable `ThumbnailsPanel`.

use leptos::prelude::*;

use pdf_viewer::ThumbnailsPanel;
use pdf_viewer::ViewerState;

#[component]
pub(crate) fn SidebarThumbs(
    state: ViewerState,
    live: Signal<bool>,
    shown: Signal<bool>,
    outro: Signal<bool>,
) -> impl IntoView {
    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
        >
            <ThumbnailsPanel state=state live=live />
        </div>
    }
}
