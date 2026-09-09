//! The library's content area: what the page shows under its title bar.
//!
//! Three states the reader can be in and one they cannot avoid — a document
//! opening, a document that failed to open, and the shelf itself. The first two
//! are the reader's, and they are here rather than on the reader's page because
//! this route is where the app lands on launch, after a close, and after a failed
//! open: an "Opening…" that only the reader page could draw is an "Opening…"
//! nobody sees.
//!
//! This is also where the things the grid and the list share are provided: the
//! order both render, the folders at this level, and the selection mode both
//! toggle into. Deriving the order once here is what makes a drag between the two
//! views impossible to get wrong — there is only one order — and it is what lets
//! the selection bar's "All" mean everything on screen rather than everything in
//! the library. It is also the order a drop names its position by, which is the
//! reason [`visible`] is a function of the state and not a closure of a view: the
//! page and the drag have to be counting the same thing.
//!
//! The empty space of this level is a drop target too, registered here rather
//! than per view because there is one scroll container and two layouts inside it.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;
use pdf_engine::types::DocStatus;
use library_core::query;
use library_core::shelf::{ALL_SHELF, Shelf, children_of};
use library_core::sort::{self, SortKey};
use library_core::book::Book;

use crate::components::primitives::feedback::CenteredLoader;
use crate::features::library::context_menu::{LibraryContextMenu, LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::empty_state::EmptyState;
use crate::features::library::grid::GridView;
use crate::features::library::list::ListView;
use crate::features::library::selection::{LibrarySelectBar, use_select_mode};
use crate::services::library::backfill_missing;
use crate::state::AppState;

/// The id of the element a drag's empty space is: the level's own scroll
/// container, which is the one box both layouts live in.
const LEVEL_DOM_ID: &str = "library-level";

/// The order the page is showing, so a drop on a card can name the index it
/// landed at and so both layouts render one list rather than two that agree.
///
/// Provided here and read by the views, the selection bar and
/// `crate::features::library::dnd::commit`; a card cannot work the index out from
/// its own DOM without counting siblings, which is a second definition of the
/// order.
#[derive(Clone, Copy)]
pub struct ShelfOrder(pub Signal<Vec<Book>>);

/// The shelves the page is showing at this level, in the order it shows them.
///
/// Provided beside [`ShelfOrder`] and for the same reason: the grid renders the
/// folders before the books, the selection bar's "All" has to mean everything on
/// screen rather than everything in the library, and a card that derived the level
/// itself would be a second answer to "what is here".
#[derive(Clone, Copy)]
pub struct FolderOrder(pub Signal<Vec<Shelf>>);

/// How long a revealed card stays lit. Long enough to find with the eye after the
/// scroll settles, short enough that it is a pointer and not a decoration.
const REVEAL_MS: u64 = 1600;

/// Take the reader to a revealed book: find its card, center it, light it, and
/// stop lighting it.
///
/// Two animation frames before the lookup, not one. The reveal usually arrives
/// with a shelf switch, and the grid it scrolls is the one the switch mounts —
/// which does not exist yet in the frame the signal was written. The first frame
/// lets Leptos flush the new shelf, the second is the one that can find the card.
fn install_reveal(state: AppState) {
    Effect::new(move |_| {
        let Some((book_id, nonce)) = state.library.reveal.get() else {
            return;
        };
        let dom_id = format!("book-{book_id}");
        let smooth = scroll_may_animate(state);
        request_animation_frame(move || {
            request_animation_frame(move || {
                let Some(node) = by_id(&dom_id) else {
                    // Filtered out by a search, or on a shelf this is not: the
                    // light goes on for nobody, and that is better than scrolling
                    // to somewhere the book is not.
                    return;
                };
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Center);
                options.set_behavior(if smooth {
                    web_sys::ScrollBehavior::Smooth
                } else {
                    web_sys::ScrollBehavior::Auto
                });
                node.scroll_into_view_with_scroll_into_view_options(&options);
            });
        });
        // Cleared on a timer rather than by the next reveal, so a card does not
        // stay lit because the reader never asked for another one. The nonce guard
        // is what lets a second reveal of the SAME book re-light it: without it the
        // clear from the first would put out the second.
        let handle = set_timeout_with_handle(
            move || {
                state.library.reveal.update(|at| {
                    if at.as_ref().is_some_and(|(_, seen)| *seen == nonce) {
                        *at = None;
                    }
                });
            },
            Duration::from_millis(REVEAL_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });
}

/// Whether the scroll may animate. The reader's own master switch and the
/// platform's answer are two different questions with the same answer shape, and
/// either one saying no is enough: a smooth scroll under
/// `prefers-reduced-motion` is exactly the motion the preference is about.
fn scroll_may_animate(state: AppState) -> bool {
    if !state.settings.with_untracked(|s| s.animations.enabled) {
        return false;
    }
    web_sys::window()
        .and_then(|window| window.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
        .is_some_and(|query| !query.matches())
}

/// The books the page shows, in the order it shows them.
///
/// This is the order a drop counts, as well as the one both layouts render: a
/// card lands "here" at an index in what the reader is looking at, and an index
/// into any other list would be a position in a shelf nobody can see.
///
/// Four steps, and the sequence is the point: the level narrows the list, the
/// view's sort orders it, and the query filters it LAST — so a search never
/// re-orders anything and clearing one puts the shelf back exactly as it was. A
/// shelf's own member order is read with `SortKey::Manual`, which is a no-op,
/// because the sort the reader chose is applied to the whole list once rather
/// than to each shelf's copy of it.
///
/// At the root the level is the TOP of the library rather than a flattening of
/// it: the books nobody has filed, beside the folders [`visible_folders`] puts
/// there. A book inside a folder is that folder's to show, and showing it at the
/// root as well was the same book on two levels at once — a flat shelf list and a
/// nested one wearing one page. A query is the one exception: searching from the
/// root searches the LIBRARY, because a search that could not see inside folders
/// would miss silently, and the matches it shows are the ones asked for.
pub(crate) fn visible(state: AppState) -> Vec<Book> {
    let view = state.library.view.get();
    let shelf_id = state.library.shelf.get();
    let books = state.library.books.get();
    let mut list = if shelf_id == ALL_SHELF {
        let terms = state.library.query.get();
        if query::is_active(&terms) {
            books
        } else {
            let filed: HashSet<String> = state.library.shelves.with(|shelves| {
                shelves
                    .iter()
                    .flat_map(|s| s.books.iter().cloned())
                    .collect()
            });
            books
                .into_iter()
                .filter(|b| !filed.contains(&b.id))
                .collect()
        }
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

/// The folders the page shows at this level, in the order it shows them.
///
/// Two steps, and the same sequence as [`visible`]: the level narrows the list to
/// the shelves filed directly inside it, and the query filters what is left — by
/// name, through the same rule the books go through, so one text box is not two
/// searches wearing one field.
///
/// The query is read BEFORE the shelves and both are read inside the one derive,
/// so a shelf created while a search is open re-runs the whole filter on the
/// frame it lands: one invalidation covers both facts, and a fresh shelf can
/// never be memoised past the query it was born under.
///
/// The view's sort key is NOT applied. It sorts books (title, author, how far in
/// they were read) and a shelf has none of those; folders render in the order the
/// library stores them, which is the order the reader made them in and the order a
/// drag between them can rewrite.
fn visible_folders(state: AppState) -> Vec<Shelf> {
    let at = state.library.shelf.get();
    let terms = state.library.query.get();
    // "All" is not a shelf, so the root level is the shelves with no parent
    // rather than the shelves whose parent is named "all".
    let parent = (at != ALL_SHELF).then_some(at.as_str());
    state.library.shelves.with(|shelves| {
        children_of(shelves, parent)
            .into_iter()
            .filter(|s| s.id != ALL_SHELF)
            .filter(|s| !query::is_active(&terms) || query::matches_terms(&s.name, &terms))
            .cloned()
            .collect()
    })
}

#[component]
pub(crate) fn LibraryContent(state: AppState) -> impl IntoView {
    // Provided before anything below can ask: both views and every card read
    // these, and a card that derived its own order would be a second definition
    // of where a drop lands. One derived signal, so the grid, the list and the
    // "nothing here" line below all agree about what is on screen.
    let order = Signal::derive(move || visible(state));
    provide_context(ShelfOrder(order));
    let folders = Signal::derive(move || visible_folders(state));
    provide_context(FolderOrder(folders));
    // The level's own empty space, registered before anything that sits in it:
    // the registry hit-tests in reverse, so the cards a shelf mounts after this
    // are found ahead of the space they stand on. One registration for both
    // layouts, because there is one scroll container and the grid and the list are
    // two things inside it.
    let drag = use_context::<DragController>().expect("the library page installs the drag session");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Level, String::new()),
        dom_id: LEVEL_DOM_ID.to_string(),
        shelf: None,
    });
    // The shelf asks for the covers it is missing whenever it is looked at. The
    // queue already skips what it has, so this reads as a question rather than a
    // command, and a render that failed for a second's reason — or a book that
    // arrived while the queue was drained elsewhere — converges on the next
    // visit instead of never. No tracked reads inside, so it asks once per mount.
    Effect::new(move |_| {
        backfill_missing(state);
    });
    install_reveal(state);
    // The exit paths for a selection a card started: Escape, a click on empty
    // shelf, and leaving the page. Installed here rather than per card because
    // those are facts about the shelf, and one listener per card would be N
    // listeners racing to leave the same mode.
    use_select_mode(state);

    let status = state.reader.document.status;
    let error = state.reader.document.error;
    let is_list = Signal::derive(move || state.library.view.with(|v| v.is_list()));
    let has_books = Signal::derive(move || state.library.books.with(|b| !b.is_empty()));
    // The page is worth drawing when there is anything on it, and a folder with no
    // books in it yet is something: an empty library that still had shelves would
    // otherwise be an empty state covering the folders the reader just made.
    let has_anything = Signal::derive(move || has_books.get() || !folders.get().is_empty());
    // A library with books in it and nothing on screen means the reader narrowed
    // it away — which is a different sentence from an empty library, and one that
    // should not offer an import button.
    let quiet = Signal::derive(move || {
        if !order.get().is_empty() || !folders.get().is_empty() || !has_books.get() {
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
                    when=move || has_anything.get()
                    fallback=move || view! { <EmptyState state=state /> }
                >
                    <div
                        id=LEVEL_DOM_ID
                        class="min-h-0 flex-1 overflow-y-auto pt-12"
                        // Empty shelf is a surface with a menu of its own: it is
                        // where "new shelf" and "select all" belong, and a card's
                        // own right-click stops propagating so this only ever hears
                        // the space between them.
                        on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                            ev.prevent_default();
                            menu.ask(
                                ev.client_x() as f64,
                                ev.client_y() as f64,
                                MenuTarget::Level,
                            );
                        }
                    >
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
            // Fixed to the viewport, and mounted whatever the shelf is doing: a
            // selection outlives the "Opening…" that can cover the grid, and a bar
            // that vanished mid-open would leave the reader in a mode with no way
            // out of it on screen.
            <LibrarySelectBar state=state />
            // Mounted here rather than on the page so the menu can read the level's
            // own order: "select all" has to mean all of what is on screen, and
            // that is the same derived list both layouts render.
            <LibraryContextMenu state=state />
        </div>
    }
}
