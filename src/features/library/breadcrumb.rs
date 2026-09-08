//! The breadcrumb: the library page's only prose, and it is navigation.
//!
//! Two crumbs at most — `All`, and the shelf the page is drilled into. "All" is
//! always a button (it is the way back) and the shelf is never one (there is
//! nowhere below it to go), which is also why the chevron points one way.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::shelf::ALL_SHELF;

use crate::state::AppState;

/// The name of the shelf the page is drilled into, or `None` at the root.
/// Tracked: the breadcrumb is a view, and a drill in or out is the only thing
/// that changes it.
fn crumb(state: AppState) -> Signal<Option<String>> {
    Signal::derive(move || {
        let id = state.library.shelf.get();
        if id == ALL_SHELF {
            return None;
        }
        // A shelf the list no longer holds answers as the root rather than as a
        // crumb with no name in it. Nothing removes a shelf while the page is up
        // today (only a load-time sanitize can, and the drilled-into shelf is not
        // persisted), so this is the belt and not the mechanism.
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
        })
    })
}

#[component]
pub(crate) fn Breadcrumb(state: AppState) -> impl IntoView {
    let name = crumb(state);
    view! {
        <nav class="flex min-w-0 items-center gap-0.5 text-sm" aria-label="Library location">
            <button
                type="button"
                title="Every book"
                on:click=move |_| state.library.shelf.set(ALL_SHELF.to_string())
                class=move || {
                    // The crumb that is not where you are reads as a way back,
                    // and the one that is reads as a heading.
                    let base = "shrink-0 rounded-md px-1.5 py-0.5 transition-colors \
                                focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                    if name.get().is_some() {
                        format!("{base} text-muted hover:bg-line hover:text-ink")
                    } else {
                        format!("{base} font-medium text-ink")
                    }
                }
            >
                "All"
            </button>
            {move || {
                name.get().map(|shelf| {
                    let tooltip = shelf.clone();
                    view! {
                        <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
                        <span class="truncate font-medium text-ink" title=tooltip>
                            {shelf}
                        </span>
                    }
                })
            }}
        </nav>
    }
}
