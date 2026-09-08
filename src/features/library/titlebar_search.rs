//! The library's search bar, in the title bar's centre slot.
//!
//! The reader's floating search is an overlay over a document and behaves like
//! one (it opens on a shortcut, it dismisses on an outside press, it owns a
//! results list). This is a filter over a shelf, so it borrows the LOOK and
//! nothing else: an always-present pill in the bar's centre slot, typing into it
//! narrows the grid on the spot, and `Escape` clears it rather than closing it —
//! a control that is always on the page cannot be dismissed.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::events::FOCUS_LIBRARY_SEARCH_EVENT;
use crate::state::AppState;

/// The bar's placeholder, which is also its only claim about the library: how
/// many books a query is being run against.
fn placeholder(state: AppState) -> Signal<String> {
    Signal::derive(move || {
        let books = state.library.books.with(|b| b.len());
        match books {
            0 => "Search the library".to_string(),
            1 => "Search 1 book".to_string(),
            n => format!("Search {n} books"),
        }
    })
}

#[component]
pub(crate) fn TitlebarSearch(state: AppState) -> impl IntoView {
    let hint = placeholder(state);
    let has_query = Signal::derive(move || state.library.query.with(|q| !q.is_empty()));
    let input_ref: NodeRef<html::Input> = NodeRef::new();

    // Cmd/Ctrl+F lands here while the shelf is what is on screen. The shortcut
    // layer dispatches and forgets: it has no business knowing that the library's
    // bar owns an input node, and the bar has no business owning a key binding.
    let focus_handle =
        window_event_listener(
            leptos::ev::Custom::new(FOCUS_LIBRARY_SEARCH_EVENT),
            move |_: web_sys::CustomEvent| {
                if let Some(node) = input_ref.get_untracked() {
                    _ = node.focus();
                    node.select();
                }
            },
        );
    on_cleanup(move || focus_handle.remove());

    view! {
        <div
            class="pointer-events-auto flex w-full max-w-xl items-center gap-2 rounded-full \
                   border border-line bg-surface/70 px-3 py-1.5 backdrop-blur \
                   focus-within:border-accent"
        >
            <Icon name=IconName::Search size=15 class="shrink-0 text-muted" />
            <input
                node_ref=input_ref
                type="text"
                aria-label="Search the library"
                placeholder=move || hint.get()
                prop:value=move || state.library.query.get()
                on:input=move |ev| state.library.query.set(event_target_value(&ev))
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    // The global shortcuts stand down inside a form control, so
                    // this bar owns its own Escape. Clearing is the only thing
                    // Escape means here: the bar itself never closes.
                    if ev.key() == "Escape" && has_query.get_untracked() {
                        ev.stop_propagation();
                        state.library.query.set(String::new());
                    }
                }
                class="w-full min-w-0 bg-transparent text-sm text-ink placeholder:text-muted \
                       focus:outline-none"
            />
            {move || {
                has_query.get().then(|| {
                    view! {
                        <button
                            type="button"
                            aria-label="Clear the search"
                            title="Clear the search"
                            on:click=move |_| state.library.query.set(String::new())
                            class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full \
                                   text-muted transition-colors hover:bg-line hover:text-ink \
                                   focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        >
                            <Icon name=IconName::Close size=12 />
                        </button>
                    }
                })
            }}
        </div>
    }
}
