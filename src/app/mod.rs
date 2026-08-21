//! Application root: installs the storage backend, boots the persisted
//! state, provides the app contexts, installs the app-wide effects, and
//! mounts the routed shell.

mod bootstrap;
mod routes;
mod shell;

use leptos::prelude::*;
use leptos_router::components::Router;

use crate::components::overlays::toast::ToastHost;
use crate::effects::link_navigation::link_navigation;
use crate::effects::page_selection::page_selection;
use crate::effects::theme::apply_theme;
use crate::state::AppState;
use bootstrap::{create_app_state, install_storage, provide_app_contexts};
use shell::AppShell;

#[component]
pub fn App() -> impl IntoView {
    // Storage backend: localStorage today. One line to swap in bootstrap.
    install_storage();

    let state = create_app_state();
    provide_context(state);
    provide_app_contexts(state);

    // App-root hooks: theme (both pages), global keyboard shortcuts, internal
    // PDF link jumps, and text-selection page-range tracking.
    apply_theme(state);
    shortcuts(state);
    link_navigation(state);
    page_selection(state);
    // OS file opening: double-click / "Open with" / default-app launch.
    crate::state::open::init_open_file_handling(state);

    view! {
        <Router>
            <AppShell state=state />
        </Router>
        // App-root toast host: fixed overlay, safe outside the toolbar's
        // backdrop-blur stacking context.
        <ToastHost state=state />
    }
}

/// Global keyboard shortcuts; the open-file action is injected from the app so
/// the viewer crate never depends on app chrome.
fn shortcuts(state: AppState) {
    let open_doc = {
        move || crate::state::open::open_dialog(state)
    };
    pdf_viewer::effects::shortcuts::shortcuts(
        state.viewer_state(),
        open_doc,
    );
}
