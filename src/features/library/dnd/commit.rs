//! The only place a drop touches library state.
//!
//! Every move here rides a service the shelf's menus already ride —
//! `crate::services::library::arrange` for the moves and
//! `crate::services::library::create_shelf` for the one a fold makes — so a
//! dragged book persists, keeps its cover and is revealed exactly as a filed one
//! is. Nothing in here decides anything either: the decision arrived as a
//! [`DropEffect`] and what is left is which operation it names.
//!
//! One rule carries over from the services and is worth repeating at the seam
//! where a reader's hand meets it: an in-app move never touches the filesystem.
//! A drop on the root crumb takes a book OFF the shelf it was on; it does not
//! move a file out of a folder on disk, because the book was never in one the app
//! owns.

use leptos::prelude::*;

use library_core::shelf::{self, ALL_SHELF};

use super::controller::DragPayload;
use super::effect::DropEffect;
use crate::services::library::{
    create_shelf, move_many_to_shelf, nest_many, nest_shelf, reorder_shelves_to_anchor,
    unfile_books,
};
use crate::state::AppState;

/// Do what `effect` says, with what the reader was holding.
pub fn apply(state: AppState, effect: DropEffect, payload: DragPayload) {
    if payload.is_empty() {
        return;
    }
    // What the move takes its books OFF: the shelf whose member list rendered
    // the row the press began on, when the row named one. A drag inside an
    // expanded branch is that branch's — reading the page's level here instead
    // would unfile a book that sits on both from the open shelf for a reorder
    // that never left the branch. A lift with no named container (a grid card,
    // a flat row) belongs to the open level, and `None` at the root means "no
    // shelf": the library's own order has no member list to take a book off.
    let from = match payload.source.clone() {
        Some(named) => shelf::level_of_owned(named.as_str()),
        None => {
            let open = state.library.shelf.get_untracked();
            shelf::level_of_owned(&open)
        }
    };

    match effect {
        DropEffect::Refused => {}
        DropEffect::InsertBefore {
            book_id,
            shelf,
            after,
        } => {
            // A drop on a row means "put it here", and HERE is two facts the
            // effect carries rather than this step re-deriving: the container
            // that renders the row — a nested tree row answers to its own
            // shelf, not to the level the page is on — and which side of the
            // anchor the seam was. The held folders get no position: a level
            // renders its folders before its books, in the order the library
            // stores them, and a drag that promised a place among the covers
            // would be a promise the next render breaks. They are not left in
            // the hand either — they join the container the books just landed
            // in, which is the same roof the level's empty space files under.
            let (to, index) = insert_anchor(state, &book_id, shelf.as_deref(), after);
            move_many_to_shelf(state, &payload.books, from, to.clone(), index);
            if to == ALL_SHELF {
                for folder in &payload.folders {
                    nest_shelf(state, folder, None);
                }
            } else {
                nest_many(state, &payload.folders, &to);
            }
        }
        DropEffect::ShelfSibling { anchor_id, after } => {
            // Folders alone on a shelf row's edge: the same level, a new place
            // in it. The books' half cannot arrive here — the table sends a
            // mixed hold inside the folder instead — so this is the folders.
            reorder_shelves_to_anchor(state, &payload.folders, &anchor_id, after);
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

/// Where an insertion lands: the shelf whose member list renders the anchor
/// row — the effect's own when the row named one, the open level otherwise —
/// and the anchor's index inside it, one further on when the seam was the row's
/// bottom edge.
///
/// The index is the anchor's position in its CONTAINER rather than a count of
/// what is on screen, and that is the fix for a drop between nested rows: an
/// expanded tree renders a shelf's members under it while the page's visible
/// order is the flat section's, so a screen count would name a slot in the
/// wrong list. A count of the container is also the count the search cannot
/// skew: the filtered page shows fewer rows than the member list holds, and a
/// filtered index applied to an unfiltered list lands where nobody pointed.
///
/// `None` while the view is sorted rather than manual: a sorted level re-sorts
/// on the next render, so a drop that named a slot would be undone before the
/// reader saw it land. `library_core::view`'s `drag_reorders` is the question,
/// and it is the same one the grid asked before it made a card draggable — a
/// sorted shelf still accepts the drop, it just appends.
fn insert_anchor(
    state: AppState,
    book_id: &str,
    shelf: Option<&str>,
    after: bool,
) -> (String, Option<usize>) {
    let reorder = state.library.view.with_untracked(|view| view.drag_reorders());
    let open = state.library.shelf.get_untracked();
    let container: Option<String> = match shelf {
        // A row that named its shelf. The root spells itself "all" and is the
        // library's own order rather than a member list.
        Some(named) => shelf::level_of_owned(named),
        None => shelf::level_of_owned(&open),
    };
    let step = usize::from(after && reorder);
    match container {
        Some(id) => {
            let index = reorder.then(|| {
                state.library.shelves.with_untracked(|shelves| {
                    shelves
                        .iter()
                        .find(|each| each.id == id)
                        .and_then(|each| each.books.iter().position(|member| member == book_id))
                        .map_or(0, |at| at + step)
                })
            });
            (id, index)
        }
        None => {
            let index = reorder.then(|| {
                state.library.books.with_untracked(|rows| {
                    rows.iter()
                        .position(|row| row.id() == book_id)
                        .map_or(0, |at| at + step)
                })
            });
            (ALL_SHELF.to_string(), index)
        }
    }
}
