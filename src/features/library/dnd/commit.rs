//! The only place a drop touches library state.
//!
//! Every move here rides a service the shelf's menus already ride —
//! `crate::services::library::arrange` for the moves and
//! `crate::services::library::create_shelf` for the one a fold makes — so a
//! dragged book persists, keeps its cover and is revealed exactly as a filed one
//! is. Nothing in here decides anything either: the decision arrived as a
//! [`DropEffect`] and what is left is which of five operations it names.
//!
//! One rule carries over from the services and is worth repeating at the seam
//! where a reader's hand meets it: an in-app move never touches the filesystem.
//! A drop on the root crumb takes a book OFF the shelf it was on; it does not
//! move a file out of a folder on disk, because the book was never in one the app
//! owns.

use leptos::prelude::*;

use library_core::shelf::ALL_SHELF;

use super::controller::DragPayload;
use super::effect::DropEffect;
use crate::features::library::content::visible;
use crate::services::library::{
    create_shelf, move_many_to_shelf, nest_many, nest_shelf, unfile_books,
};
use crate::state::AppState;

/// Do what `effect` says, with what the reader was holding.
pub fn apply(state: AppState, effect: DropEffect, payload: DragPayload) {
    if payload.is_empty() {
        return;
    }
    // The level the drag started on. `None` at the root, which is not a shelf
    // and so has no member list to take a book off.
    let open = state.library.shelf.get_untracked();
    let from = (open != ALL_SHELF).then_some(open);

    match effect {
        DropEffect::Refused => {}
        DropEffect::InsertBefore { book_id } => {
            // A drop on a card means "put it here", which is a position in the
            // order the reader is looking at. The held folders get no position:
            // a level renders its folders before its books, in the order the
            // library stores them, and a drag that promised a place among the
            // covers would be a promise the next render breaks.
            let to = from.clone().unwrap_or_else(|| ALL_SHELF.to_string());
            let index = index_of(state, &book_id);
            move_many_to_shelf(state, &payload.books, from, to, index);
        }
        DropEffect::FileToShelf { shelf_id } if shelf_id.is_empty() => {
            // The root, which is a level and not a shelf. From inside a shelf
            // this is the way a book comes OUT of it; from the root already it is
            // the empty space at the end of the library's own order.
            match from.as_deref() {
                Some(shelf) => unfile_books(state, &payload.books, shelf),
                None => {
                    move_many_to_shelf(state, &payload.books, None, ALL_SHELF.to_string(), None)
                }
            }
            for folder in &payload.folders {
                nest_shelf(state, folder, None);
            }
        }
        DropEffect::FileToShelf { shelf_id } => {
            move_many_to_shelf(state, &payload.books, from, shelf_id.clone(), None);
            nest_many(state, &payload.folders, &shelf_id);
        }
        DropEffect::NestInto { folder_id } => {
            move_many_to_shelf(state, &payload.books, from, folder_id.clone(), None);
            nest_many(state, &payload.folders, &folder_id);
        }
        DropEffect::CreateFolder { with_book_id } => {
            // Made at the level the reader is looking at, which is
            // `create_shelf`'s own rule: a shelf folded together inside a folder
            // subdivides it, and one folded together at the root is a new top
            // level. Named "New shelf" and left there, for the reason that
            // service gives — the crumb that names it is one keystroke from a
            // rename and shows the shelf it is naming.
            let shelf_id = create_shelf(state);
            let mut books = payload.books;
            if !books.contains(&with_book_id) {
                books.push(with_book_id);
            }
            move_many_to_shelf(state, &books, from, shelf_id.clone(), None);
            nest_many(state, &payload.folders, &shelf_id);
        }
    }
}

/// The index a drop on `book_id` names, or `None` for "the end of the level".
///
/// `None` while the shelf is sorted rather than a position that will not survive:
/// a sorted level re-sorts on the next render, so a drop that named a slot would
/// be undone before the reader saw it land. `library_core::view`'s
/// `drag_reorders` is the question, and it is the same one the grid used to ask
/// before it made a card draggable.
///
/// The order is [`visible`] — the one the page is showing. A card cannot work its
/// own position out from the DOM without counting siblings, and a count of
/// siblings would be a second definition of an order the search and the sort have
/// already had their say about.
fn index_of(state: AppState, book_id: &str) -> Option<usize> {
    if !state.library.view.with_untracked(|view| view.drag_reorders()) {
        return None;
    }
    visible(state)
        .iter()
        .position(|book| book.id == book_id)
}
