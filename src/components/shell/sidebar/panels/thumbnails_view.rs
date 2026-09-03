//! Thumbnails panel host: the absolutely-stacked panel wrapper with its
//! paint/outro toggles, around the reusable `ThumbnailsPanel`.

use leptos::prelude::*;

use crate::components::shell::sidebar::panels::thumbnails::ThumbnailsPanel;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;
use leptos::prelude::RwSignal;

#[component]
pub(crate) fn SidebarThumbs(
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    live: Signal<bool>,
    shown: Signal<bool>,
    outro: Signal<bool>,
    intro: Signal<bool>,
) -> impl IntoView {
    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
            class=("is-intro", move || intro.get())
        >
            // The engine owns thumbnails; a text document never reaches it,
            // so the panel mounts nothing for one (the rail's redirect keeps
            // it un-shown besides).
            <Show when=move || !state.document.format.get().is_text()>
                <ThumbnailsPanel state=state live=live sidebar=sidebar />
            </Show>
        </div>
    }
}
