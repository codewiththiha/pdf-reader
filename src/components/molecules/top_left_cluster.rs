//! Persistent top-left chrome for the sidebar-CLOSED state: the native traffic
//! lights (reserved via left padding, painted by the OS) plus a single sidebar
//! toggler. Renders nothing while the sidebar is open — the sidebar's own
//! chrome row owns that band then (see organisms::sidebar).

use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::IconName;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_viewer::state::SidebarMode;
use crate::core::state::AppState;

#[component]
pub fn TopLeftCluster(state: AppState) -> impl IntoView {
    view! {
        <Show when=move || state.sidebar.get() == SidebarMode::None>
            // h-12 matches the traffic-light Y set in tauri.conf.json.
            // pl reserves the native lights' width so the button never sits
            // under them. Draggable like the rest of the top edge.
            <div
                class="absolute left-0 top-0 z-40 flex h-12 items-center pl-[76px] pr-2"
                data-tauri-drag-region="true"
            >
                <Tooltip text="Toggle sidebar".to_string()>
                    <Button
                        on_click=move |_| state.sidebar.set(SidebarMode::Thumbs)
                        kind=ButtonKind::Ghost
                        icon=IconName::Menu
                        title="Toggle sidebar".to_string()
                    />
                </Tooltip>
            </div>
        </Show>
    }
}
