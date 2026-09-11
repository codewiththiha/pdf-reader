//! Taking the reader to a book.
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

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use library_core::shelf::{ALL_SHELF, containing};

use crate::state::AppState;

/// Monotonic, so two reveals in the same millisecond are still two reveals.
static NONCE: AtomicU64 = AtomicU64::new(1);

/// Go to a book: its shelf, then the card itself.
pub fn reveal_book(state: AppState, book_id: &str) {
    navigate_to_shelf_of(state, book_id);
    state.library.reveal.set(Some((
        book_id.to_string(),
        NONCE.fetch_add(1, Ordering::Relaxed),
    )));
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
        .with_untracked(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == shelf_id)
                .and_then(|s| s.parent.clone())
        })
        .unwrap_or_else(|| ALL_SHELF.to_string());
    if state.library.shelf.get_untracked() != level {
        state.library.shelf.set(level);
    }
    state.library.reveal.set(Some((
        shelf_id.to_string(),
        NONCE.fetch_add(1, Ordering::Relaxed),
    )));
}

/// Put the breadcrumb under the shelf a book is filed on — the FIRST one, in
/// shelf order, so the answer is the same every time — or at the root when it is
/// on no shelf. A book in the library and on nothing is in "All", and "All" is
/// where the reader will find it.
pub fn navigate_to_shelf_of(state: AppState, book_id: &str) {
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
