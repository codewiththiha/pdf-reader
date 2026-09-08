//! A bookshelf inside the shelf: one shelf's name, a strip of its covers, and
//! the board they stand on.
//!
//! The strip is the whole idea — a shelf is recognised by the spines on it, not
//! by its name — so the covers overlap and fan out on hover, and the tile reads
//! as a row of books on a board rather than as a card that happens to contain
//! thumbnails. Everything it does beyond that is a count and a click.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{Shelf, ALL_SHELF};

use crate::features::library::drag::{self, DropTarget};
use crate::services::library::move_to_shelf;
use crate::state::AppState;

/// How many covers the strip shows before it says "+n" instead. Past this the
/// strip is a hint nobody reads, and the count next to the name is the answer.
const STRIP_CAP: usize = 8;

/// The drop target's namespace for a tile. One signal drives both markers — the
/// insertion line a card draws and the lift a tile draws — so the two have to be
/// distinguishable by value, and a book id is never prefixed.
const TILE_TARGET: &str = "shelf:";

#[component]
pub(crate) fn ShelfTile(state: AppState, shelf: Shelf) -> impl IntoView {
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");

    // One owned copy per closure: the tile renders eight of them and each
    // outlives this frame.
    let head_name = shelf.name.clone();
    let head_title = shelf.name.clone();
    let aria_name = shelf.name.clone();
    let click_id = shelf.id.clone();
    let key_id = shelf.id.clone();
    let drop_id = shelf.id.clone();
    let strip_members = shelf.books.clone();
    let count_members = shelf.books.clone();
    let tile_target = format!("{TILE_TARGET}{}", shelf.id);
    let over_target = tile_target.clone();
    let leave_target = tile_target.clone();
    // Two different facts, and the tile shows both: that a shelf was cut from a
    // folder (the glyph, so a reader knows where these books live on disk), and
    // that the folder is still being watched (the dot, so a reader knows the
    // shelf may fill itself). The second needs the folder row, so it is a
    // reactive read rather than a copy taken at mount.
    let from_folder = shelf.is_folder();
    let watched_folder = shelf.kind.folder_id().map(str::to_string);
    let watched = Signal::derive(move || {
        let Some(folder_id) = watched_folder.clone() else {
            return false;
        };
        state.library.folders.with(|folders| {
            folders
                .iter()
                .any(|f| f.id == folder_id && f.opts.watch)
        })
    });

    // The strip's covers, resolved against the library. A member that names no
    // book is dropped rather than rendered as a blank spine.
    let strip = Signal::derive(move || {
        state.library.books.with(|books| {
            strip_members
                .iter()
                .filter_map(|id| books.iter().find(|b| &b.id == id))
                .take(STRIP_CAP)
                .cloned()
                .collect::<Vec<Book>>()
        })
    });
    let count = Signal::derive(move || count_members.len());
    let overflow = Signal::derive(move || count.get().saturating_sub(STRIP_CAP));

    view! {
        <div
            class="shelf-tile"
            class=("shelf-tile-over", move || {
                drop_target
                    .0
                    .with(|t| t.as_deref() == Some(over_target.as_str()))
            })
            role="button"
            tabindex="0"
            aria-label=move || format!("Open the {aria_name} shelf")
            on:click=move |_| state.library.shelf.set(click_id.clone())
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() == "Enter" {
                    state.library.shelf.set(key_id.clone());
                }
            }
            on:dragover=move |ev| {
                if drag::accept(&ev) {
                    // A shelf is one target, not a position in a list: the line
                    // a card draws would only suggest an index this drop does not
                    // honour, so the tile claims the marker under its own name.
                    drop_target.0.set(Some(tile_target.clone()));
                }
            }
            on:dragleave=move |_| {
                drop_target.0.update(|at| {
                    if at.as_deref() == Some(leave_target.as_str()) {
                        *at = None;
                    }
                });
            }
            on:drop=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                drop_target.0.set(None);
                let Some(dragged) = drag::dragged(&ev) else {
                    return;
                };
                let from = state.library.shelf.get_untracked();
                let source = if from == ALL_SHELF {
                    None
                } else {
                    Some(from)
                };
                move_to_shelf(state, dragged, source, drop_id.clone(), None);
            }
        >
            <div class="shelf-head">
                <span class="shelf-name" title=head_title.clone()>{head_name}</span>
                {from_folder.then(|| {
                    view! {
                        <span class="shelf-watch" title="Cut from a folder on disk">
                            <Icon name=IconName::Open size=11 />
                        </span>
                    }
                })}
                {move || {
                    watched.get().then(|| {
                        view! {
                            <span class="shelf-watch-dot" title="Watched for new books"></span>
                        }
                    })
                }}
                <span class="shelf-count">
                    {move || {
                        let books = count.get();
                        if books == 1 {
                            "1 book".to_string()
                        } else {
                            format!("{books} books")
                        }
                    }}
                </span>
                <Icon name=IconName::Next size=14 class="ml-auto shrink-0 text-muted" />
            </div>
            <div class="shelf-strip">
                {move || {
                    strip
                        .get()
                        .into_iter()
                        .map(|book| view! { <Spine state=state book=book /> })
                        .collect_view()
                }}
                {move || {
                    (overflow.get() > 0).then(|| {
                        view! {
                            <span class="shelf-more">{format!("+{}", overflow.get())}</span>
                        }
                    })
                }}
            </div>
        </div>
    }
}

/// One cover on the strip: the cached art when there is any, the title's initial
/// on a themed plate when there is not. Smaller and quieter than a card's cover
/// — a strip is a hint, not the shelf itself.
#[component]
fn Spine(state: AppState, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let alt = book.title();
    let initial = alt
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .collect::<String>();
    view! {
        <span class="shelf-spine">
            {move || {
                match state
                    .library
                    .covers
                    .with(|covers| covers.get(&path).cloned())
                {
                    Some(cover) => {
                        view! {
                            <img
                                class="shelf-spine-img"
                                src=cover.data_url.clone()
                                alt=alt.clone()
                                loading="lazy"
                            />
                        }
                            .into_any()
                    }
                    None => {
                        view! { <span class="shelf-spine-fallback">{initial.clone()}</span> }
                            .into_any()
                    }
                }
            }}
        </span>
    }
}
