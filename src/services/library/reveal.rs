//! Taking the reader to a book — in the library, and on the disk.
//!
//! Two halves, and the order is the whole of it: the breadcrumb moves to the shelf
//! the book is on, and only then does the card get scrolled to and lit up. Doing
//! it the other way round would scroll a grid that is about to be replaced, and
//! the reader would watch the page jump to somewhere they are no longer.
//!
//! The signal carries a nonce beside the id because "reveal this book" is a
//! gesture, not a state: revealing the same book twice in a row has to work twice,
//! and a plain `Option<String>` would be unchanged by the second one and so would
//! notify nobody.
//!
//! The third door is the OS's: [`reveal_in_folder`] hands a path to the file
//! manager, and the two resolvers beside it answer WHICH path a row or a
//! shelf is — the store's own copy for a book the library owns, the file
//! where it stands for one read at its place, the directory its tree cut it
//! from for a watched folder's shelf.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use library_core::book::Row;
use library_core::folder::dir_of_rung;
use library_core::shelf::{ALL_SHELF, ShelfKind, containing, find};

use crate::state::library::Reveal;
use crate::state::{AppState, Toast};

/// Monotonic, so two reveals in the same millisecond are still two reveals.
static NONCE: AtomicU64 = AtomicU64::new(1);

/// Go to a book: its shelf, then the card itself.
pub fn reveal_book(state: AppState, book_id: &str) {
    navigate_to_shelf_of(state, book_id);
    light(state, book_id);
}

/// The one write a reveal is: what to light, and the nonce that makes a second
/// reveal of the SAME thing a second reveal. A book's reveal and a shelf's go
/// through it, so the six surfaces asking
/// [`crate::state::library::LibraryState::is_revealed`] read one shape and the
/// nonce stays this module's business rather than theirs.
fn light(state: AppState, id: &str) {
    state.library.reveal.set(Some(Reveal {
        id: id.to_string(),
        nonce: NONCE.fetch_add(1, Ordering::Relaxed),
    }));
}

/// Go to a shelf: the level it hangs on, then the folder itself, lit.
///
/// The shelf half of [`reveal_book`] and where a folder link's tap goes: the
/// pointer promises "opens the folder where it is", and where it is may be a
/// level the reader is not on. The light is the same signal and the same
/// nonce — a shelf id is a letter apart from a book id, so the surfaces can
/// tell whose reveal is whose.
pub fn reveal_shelf(state: AppState, shelf_id: &str) {
    let level = state
        .library
        .shelves
        .with_untracked(|shelves| find(shelves, shelf_id).and_then(|s| s.parent.clone()))
        .unwrap_or_else(|| ALL_SHELF.to_string());
    if state.library.shelf.get_untracked() != level {
        state.library.shelf.set(level);
    }
    light(state, shelf_id);
}

/// Put the breadcrumb under the shelf a book is filed on — the FIRST one, in
/// shelf order, so the answer is the same every time — or at the root when it is
/// on no shelf. A book in the library and on nothing is in "All", and "All" is
/// where the reader will find it.
fn navigate_to_shelf_of(state: AppState, book_id: &str) {
    let target = state
        .library
        .shelves
        .with_untracked(|shelves| {
            containing(shelves, book_id)
                .first()
                .map(|s| s.id.clone())
        })
        .unwrap_or_else(|| ALL_SHELF.to_string());
    if state.library.shelf.get_untracked() != target {
        state.library.shelf.set(target);
    }
}

/// The address a ROW reveals in the OS file manager: the store's own file for
/// a book the library copied — the copy IS the file this row reads — and the
/// file where it stands for a book read at its place. One answer, because
/// `Book::path` is the one function that says where a book reads from, and
/// the reader, the cover queue and the removal receipt all give the same.
///
/// A link reveals what it points AT, by the pointer's own rule: a book by
/// this rule, a shelf by [`path_of_shelf`]. A link at nothing — which
/// `drop_dead_shelf_links` and the book's own sweep make unreachable, but a
/// blob caught between two writes can still carry — reveals nothing, and the
/// menu simply does not offer the row.
pub fn path_of_row(state: AppState, row_id: &str) -> Option<String> {
    match state.library.row(row_id)? {
        Row::Book(book) => Some(book.path().to_string()),
        Row::Link { target, .. } => {
            if library_core::id::is_shelf(&target) {
                path_of_shelf(state, &target)
            } else {
                match state.library.row(&target)? {
                    Row::Book(book) => Some(book.path().to_string()),
                    Row::Link { .. } => None,
                }
            }
        }
    }
}

/// The directory a SHELF reveals: the ground its watched folder's tree cut it
/// from — the watched root with the rung's own key joined on,
/// [`dir_of_rung`]'s answer — for a folder's shelf, read at place or copied,
/// because that is the directory the shelf represents. A shelf the reader
/// owns has no ground and answers none.
pub fn path_of_shelf(state: AppState, shelf_id: &str) -> Option<String> {
    let shelves = state.library.shelves.get_untracked();
    let shelf = find(&shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let folder = state.library.folder(folder_id)?;
    Some(dir_of_rung(&folder.root, rel.as_deref().unwrap_or("")))
}

/// Take the reader to the file itself, in the OS's own terms: the file
/// manager opens on the item, selected inside its folder.
///
/// The OS half of this module's job: [`reveal_book`] and [`reveal_shelf`] take
/// the reader to a thing in the library, and this takes the reader to the
/// thing's ground on disk — which path a row reveals is [`path_of_row`]'s
/// answer, and the caller holds it before the ask. A failure — no shell, a
/// dead address, no file manager — is one toast and no state: a reveal is a
/// courtesy, and nothing in the library changes because one could not run.
pub fn reveal_in_folder(state: AppState, path: String) {
    if !tauri_bridge::has_tauri() {
        state.ui.toast.set(Some(Toast::new(
            "Revealing a file is only available in the desktop app.".to_string(),
        )));
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(message) = super::reveal_path(path).await {
            state.ui.toast.set(Some(Toast::new(message)));
        }
    });
}
