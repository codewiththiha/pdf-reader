//! The frameless window's caption cluster: minimize / maximize-restore /
//! close, drawn by the webview where the platform config stripped the
//! native title bar (Windows and Linux — macOS keeps its traffic lights
//! and never mounts this; `app_title_bar.rs` owns that split).
//!
//! The two frameless desktops get their platform's own shape for the
//! cluster: Windows keeps its square, full-height caption buttons
//! ([`WindowsControls`]), while Linux draws GNOME-style circles
//! ([`GnomeControls`]). [`WindowControls`] picks per process — the
//! platform is fixed for the life of the webview (probed once in
//! `services/platform.rs`), so the choice is a plain branch, not a
//! reactive one. Same three commands, same `maximized` glyph swap, same
//! position; only the shape and styling differ.
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
use crate::services::platform::is_linux;

#[component]
pub fn WindowControls(
    /// The window's live maximized state (owned by `UiState`, written by
    /// the window-state bridge). Swaps the maximize/restore glyph.
    maximized: RwSignal<bool>,
) -> impl IntoView {
    if is_linux() {
        view! { <GnomeControls maximized=maximized /> }.into_any()
    } else {
        view! { <WindowsControls maximized=maximized /> }.into_any()
    }
}

/// Fire-and-forget caption commands. Each is a silent no-op outside Tauri
/// (the probes inside `pdf_engine::api` own that), which is why plain
/// browser previews of the bar stay clickable.
fn minimize() {
    wasm_bindgen_futures::spawn_local(async move {
        pdf_engine::api::minimize_window().await;
    });
}

fn toggle_maximize() {
    wasm_bindgen_futures::spawn_local(async move {
        pdf_engine::api::toggle_maximize_window().await;
    });
}

fn close() {
    wasm_bindgen_futures::spawn_local(async move {
        pdf_engine::api::close_window().await;
    });
}

/// Square, full-height Windows-style buttons.
#[component]
fn WindowsControls(maximized: RwSignal<bool>) -> impl IntoView {
    view! {
        // NO data-tauri-drag-region in here — these must stay clickable.
        <div class="window-controls">
            <button type="button" class="win-btn" title="Minimize" aria-label="Minimize" on:click=move |_| minimize()>
                <Icon name=IconName::WindowMinimize size=14 />
            </button>

            <button
                type="button"
                class="win-btn"
                title=move || if maximized.get() { "Restore" } else { "Maximize" }
                aria-label=move || if maximized.get() { "Restore" } else { "Maximize" }
                on:click=move |_| toggle_maximize()
            >
                {move || {
                    if maximized.get() {
                        view! { <Icon name=IconName::WindowRestore size=14 /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::WindowMaximize size=14 /> }.into_any()
                    }
                }}
            </button>

            <button type="button" class="win-btn win-btn-close" title="Close" aria-label="Close" on:click=move |_| close()>
                <Icon name=IconName::Close size=14 />
            </button>
        </div>
    }
}

/// Circular GNOME-style buttons (Linux). Same three commands, same
/// `maximized` glyph swap — only the shape differs: 24px circles with a
/// translucent fill that tracks the theme ink (the style lives in
/// `styles/components/title_bar.css` under `.gnome-btn`).
#[component]
fn GnomeControls(maximized: RwSignal<bool>) -> impl IntoView {
    view! {
        // NO data-tauri-drag-region in here — these must stay clickable.
        <div class="window-controls gnome">
            <button type="button" class="gnome-btn" title="Minimize" aria-label="Minimize" on:click=move |_| minimize()>
                <Icon name=IconName::WindowMinimize size=12 />
            </button>

            <button
                type="button"
                class="gnome-btn"
                title=move || if maximized.get() { "Restore" } else { "Maximize" }
                aria-label=move || if maximized.get() { "Restore" } else { "Maximize" }
                on:click=move |_| toggle_maximize()
            >
                {move || {
                    if maximized.get() {
                        view! { <Icon name=IconName::WindowRestore size=12 /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::WindowMaximize size=12 /> }.into_any()
                    }
                }}
            </button>

            <button type="button" class="gnome-btn gnome-btn-close" title="Close" aria-label="Close" on:click=move |_| close()>
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
