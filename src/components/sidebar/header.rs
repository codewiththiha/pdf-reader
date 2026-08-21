//! Row 1 of the sidebar: close toggle, floating-search toggle, More menu.
//! Always visible while the sidebar is open (not hover-gated) — the sidebar
//! is the native traffic lights' home.

use leptos::prelude::*;

use crate::components::{Button, ButtonKind};
use crate::components::{Icon, IconName};
use crate::components::Tooltip;
use crate::state::SidebarMode;
use crate::components::MoreMenu;
use crate::state::AppState;

#[component]
pub(crate) fn SidebarHeader(state: AppState) -> impl IntoView {
    view! {
        // The chrome row carries the 48px traffic-light inset; the filled
        // panel glyph marks "sidebar is on". A drag region so the window
        // stays grabable from the sidebar.
        <div
            class="flex h-12 shrink-0 items-center gap-1 pl-[88px] pr-2"
            data-tauri-drag-region="true"
        >
            <Tooltip text="Close sidebar".to_string()>
                <Button
                    on_click=move |_| state.ui.sidebar.set(SidebarMode::None)
                    kind=ButtonKind::Ghost
                    icon=IconName::SidebarOpen
                    title="Close sidebar".to_string()
                />
            </Tooltip>
            // Floating-search toggle: raw button so pointerdown can
            // stop propagation (the floating bar's outside-click
            // dismiss listens on window pointerdown, which would
            // otherwise close the bar and let the click re-open it —
            // a one-way toggle).
            <button
                type="button"
                data-search-chrome="true"
                title="Search (Cmd/Ctrl+F)"
                on:pointerdown=move |ev| ev.stop_propagation()
                on:click=move |_| {
                    let vs = state.reader;
                    if state.reader.search.visible.get() {
                        crate::effects::search_effects::dismiss_search(vs);
                    } else {
                        crate::effects::search_effects::resume_search(vs);
                    }
                }
                class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
                <Icon name=IconName::Search size=18 />
            </button>
            <MoreMenu />
        </div>
    }
}
