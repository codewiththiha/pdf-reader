//! The library's content area: what the page shows under its title bar.
//!
//! Three states the reader can be in and one they cannot avoid — a document
//! opening, a document that failed to open, and the shelf itself. The first two
//! are the reader's, and they are here rather than on the reader's page because
//! this route is where the app lands on launch, after a close, and after a failed
//! open: an "Opening…" that only the reader page could draw is an "Opening…"
//! nobody sees.
//!
//! This is also where the two things the grid and the list share are provided:
//! the order both render, and the one drop-target signal both draw their markers
//! from. Deriving the order once here is what makes a drag between the two views
//! impossible to get wrong — there is only one order.

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use library_core::query;
use library_core::shelf::ALL_SHELF;
use library_core::sort::{self, SortKey};
use library_core::book::Book;

use crate::components::primitives::feedback::CenteredLoader;
use crate::features::library::drag::{DropTarget, ShelfOrder};
use crate::features::library::empty_state::EmptyState;
use crate::features::library::grid::GridView;
use crate::features::library::list::ListView;
use crate::state::AppState;

/// The books the page shows, in the order it shows them.
///
/// Four steps, and the sequence is the point: the drilled-into shelf narrows the
/// list, the view's sort orders it, and the query filters it LAST — so a search
/// never re-orders anything and clearing one puts the shelf back exactly as it
/// was. A shelf's own member order is read with `SortKey::Manual`, which is a
/// no-op, because the sort the reader chose is applied to the whole list once
/// rather than to each shelf's copy of it.
fn visible(state: AppState) -> Vec<Book> {
    let view = state.library.view.get();
    let shelf_id = state.library.shelf.get();
    let books = state.library.books.get();
    let mut list = if shelf_id == ALL_SHELF {
        books
    } else {
        let members = state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == shelf_id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        });
        sort::ordered(&books, &members, SortKey::Manual, true)
    };
    sort::sort_books(&mut list, view.sort, view.sort_asc);
    query::filter(&list, &state.library.query.get())
}

#[component]
pub(crate) fn LibraryContent(state: AppState) -> impl IntoView {
    // Provided before anything below can ask: both views and every card read
    // these, and a card that derived its own order would be a second definition
    // of where a drop lands. One derived signal, so the grid, the list and the
    // "nothing here" line below all agree about what is on screen.
    let order = Signal::derive(move || visible(state));
    provide_context(ShelfOrder(order));
    provide_context(DropTarget(RwSignal::new(None)));

    let status = state.reader.document.status;
    let error = state.reader.document.error;
    let is_list = Signal::derive(move || state.library.view.with(|v| v.is_list()));
    let has_books = Signal::derive(move || state.library.books.with(|b| !b.is_empty()));
    // A library with books in it and nothing on screen means the reader narrowed
    // it away — which is a different sentence from an empty library, and one that
    // should not offer an import button.
    let quiet = Signal::derive(move || {
        if !order.get().is_empty() || !has_books.get() {
            return None;
        }
        let searching = state.library.query.with(|q| !q.trim().is_empty());
        Some(if searching {
            "No books match this search.".to_string()
        } else {
            "This shelf is empty.".to_string()
        })
    });

    view! {
        <div class="flex h-full w-full flex-col">
            // The wait is the mark and nothing else. An open cannot be aborted
            // from here, and does not need to be: picking another file claims a
            // new session stamp, which is what makes the in-flight attempt drop
            // its own tail (see `crate::services::document::session`).
            <Show when=move || status.get() == DocStatus::Opening fallback=|| ()>
                <CenteredLoader />
            </Show>
            <Show when=move || status.get() == DocStatus::Error fallback=|| ()>
                <div class="flex h-full w-full items-center justify-center pt-12 text-center text-muted">
                    <p class="text-lg">
                        // The fallback names the document's own kind rather than
                        // one format: this shelf holds all three. The path is
                        // read UNTRACKED on purpose — the sentence is a snapshot
                        // of the attempt that failed, and a tracked read inside
                        // `unwrap_or_else` would only be subscribed on the runs
                        // where it happens to execute.
                        {move || {
                            error.get().unwrap_or_else(|| {
                                let kind = state
                                    .reader
                                    .document
                                    .path
                                    .get_untracked()
                                    .map_or("document", |p| {
                                        reader_core::format::format_of(&p).label()
                                    });
                                format!("Could not open this {kind}")
                            })
                        }}
                    </p>
                </div>
            </Show>
            <Show when=move || status.get() == DocStatus::Idle fallback=|| ()>
                <Show
                    when=move || has_books.get()
                    fallback=move || view! { <EmptyState state=state /> }
                >
                    <div class="min-h-0 flex-1 overflow-y-auto pt-12">
                        <div class="mx-auto w-full max-w-6xl px-6 py-8">
                            {move || {
                                if is_list.get() {
                                    view! { <ListView state=state /> }.into_any()
                                } else {
                                    view! { <GridView state=state /> }.into_any()
                                }
                            }}
                            {move || {
                                quiet.get().map(|line| {
                                    view! {
                                        <p class="py-16 text-center text-sm text-muted">{line}</p>
                                    }
                                })
                            }}
                        </div>
                    </div>
                </Show>
            </Show>
        </div>
    }
}
