//! Outline panel host: the absolutely-stacked panel wrapper with its
//! paint/outro toggles, around the reusable `OutlinePanel`.

use leptos::prelude::*;

use pdf_viewer::components::sidebar::outline::OutlinePanel;
use pdf_viewer::state::ViewerState;

#[component]
pub(crate) fn SidebarOutline(
    state: ViewerState,
    shown: Signal<bool>,
    outro: Signal<bool>,
) -> impl IntoView {
    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
        >
            <OutlinePanel state=state />
        </div>
    }
}
