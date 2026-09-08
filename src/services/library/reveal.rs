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

use library_core::shelf::ALL_SHELF;

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

/// Put the breadcrumb under the shelf a book is filed on — the FIRST one, in
/// shelf order, so the answer is the same every time — or at the root when it is
/// on no shelf. A book in the library and on nothing is in "All", and "All" is
/// where the reader will find it.
pub fn navigate_to_shelf_of(state: AppState, book_id: &str) {
    let target = state
        .library
        .shelves
        .with_untracked(|shelves| {
            shelves
                .iter()
                .find(|s| s.books.iter().any(|m| m == book_id))
                .map(|s| s.id.clone())
        })
        .unwrap_or_else(|| ALL_SHELF.to_string());
    if state.library.shelf.get_untracked() != target {
        state.library.shelf.set(target);
    }
}

#[cfg(test)]
mod tests {
    use super::NONCE;
    use std::sync::atomic::Ordering;

    #[test]
    fn two_reveals_of_one_book_are_two_reveals() {
        // The nonce is the reason a second reveal of the same book re-triggers the
        // scroll: without it the signal would hold an equal value and notify
        // nobody, and the gesture would silently do nothing the second time.
        let first = NONCE.fetch_add(1, Ordering::Relaxed);
        let second = NONCE.fetch_add(1, Ordering::Relaxed);
        assert_ne!(first, second);
    }
}
