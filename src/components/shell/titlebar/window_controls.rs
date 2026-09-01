//! The frameless window's caption cluster: minimize / maximize-restore /
//! close, drawn by the webview where the platform config stripped the
//! native title bar (Windows and Linux — macOS keeps its traffic lights
//! and never mounts this; `app_title_bar.rs` owns that split).
//!
//! The cluster lives INSIDE the toolbar row, so it shares the bar's
//! hover-reveal: like the native lights on macOS (which the bar hides
//! together with the chrome), the captions go with the bar instead of
//! floating over the page. The top hover band spans the full window width,
//! so the corner is never more than a pointer-away.
//!
//! The buttons carry no `data-tauri-drag-region`, which is what keeps them
//! clickable inside an otherwise draggable bar — Tauri only starts a drag
//! from the element that owns the attribute. Double-clicking the bar still
//! maximizes with no handler of ours in the way: Tauri's injected drag
//! script runs `internal_toggle_maximize` on the second mousedown itself,
//! and the engine's `toggle_maximize_window` is the same command behind
//! the caption button, so the two triggers can never disagree.
//!
//! The maximize/restore glyph is a PROP, not a local guess: the app-lifetime
//! window-state bridge (services/window.rs) owns the live flag, because the
//! state changes under this cluster by more than its own button — snapping,
//! drag-to-edge, taskbar restores — and a page-scoped listener would die
//! with the page that mounted it. The cluster only fires commands.

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

#[component]
pub fn WindowControls(
    /// The window's live maximized state (owned by `UiState`, written by
    /// the window-state bridge). Swaps the maximize/restore glyph.
    maximized: RwSignal<bool>,
) -> impl IntoView {
    let on_min = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::minimize_window().await;
        });
    };
    let on_toggle = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::toggle_maximize_window().await;
        });
    };
    let on_close = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::close_window().await;
        });
    };

    view! {
        // NO data-tauri-drag-region in here — these must stay clickable.
        <div class="window-controls">
            <button type="button" class="win-btn" title="Minimize" aria-label="Minimize" on:click=on_min>
                <Icon name=IconName::WindowMinimize size=14 />
            </button>

            <button
                type="button"
                class="win-btn"
                title=move || if maximized.get() { "Restore" } else { "Maximize" }
                aria-label=move || if maximized.get() { "Restore" } else { "Maximize" }
                on:click=on_toggle
            >
                {move || {
                    if maximized.get() {
                        view! { <Icon name=IconName::WindowRestore size=14 /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::WindowMaximize size=14 /> }.into_any()
                    }
                }}
            </button>

            <button type="button" class="win-btn win-btn-close" title="Close" aria-label="Close" on:click=on_close>
                <Icon name=IconName::Close size=14 />
            </button>
        </div>
    }
}
