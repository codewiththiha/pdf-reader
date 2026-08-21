//! The library route (`/`): the recent-books shelf with a minimal titlebar —
//! Open + name left, Appearance + More right, plus the built-in pin. No
//! sidebar / zoom / mode / search: those are reader-only.

use leptos::prelude::*;

use crate::components::{Button, ButtonKind};
use crate::components::IconName;
use crate::components::Tooltip;
use crate::components::TitleBar;
use crate::components::AppearanceMenu;
use crate::components::DocumentTitle;
use crate::components::MoreMenu;
use crate::state::AppState;
use crate::features::library::LibraryShelf;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                        <Button
                            on_click=move |_| crate::services::document::open_dialog(state)
                            kind=ButtonKind::Toolbar
                            icon=IconName::Open
                            label="Open".to_string()
                            title="Open PDF (Cmd/Ctrl+O)".to_string()
                        />
                    </Tooltip>
                </div>
                <DocumentTitle state=state />
            </div>
        }
    };
    let right = move || {
        view! {
            <div
                id="toolbar-right"
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <AppearanceMenu state=state />
                <MoreMenu />
            </div>
        }
    };

    view! {
        <TitleBar state=state left=left right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryShelf state=state />
            </div>
        </TitleBar>
    }
}
