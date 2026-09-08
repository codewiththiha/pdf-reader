//! The moves a reader makes by hand: a drag between shelves, a removal, a
//! relink.
//!
//! One rule covers all three, and it is why this module can be short — **an
//! in-app move never touches the filesystem.** A shelf holds book ids, so a drag
//! edits a list of ids; a read-in-place book can be filed anywhere in the app
//! without the file it points at ever being renamed, moved or copied. The only
//! byte this module deletes belongs to a book the app itself copied into its
//! store, and that goes through the shell's contained `delete_stored`.
//!
//! The rule is also what makes the ledger's hardest row true without anybody
//! having to remember it: dragging a book off a watched folder's shelf leaves
//! its fingerprint in that folder's `placed` set, so the next rescan skips it
//! instead of filing it straight back where the reader just moved it from.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::Origin;
use library_core::ledger::tombstone;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::StoreRequest;

use crate::services::library as wire;
use crate::state::library::prune_covers;
use crate::state::{AppState, Toast};

/// Move a book: onto `to` at `index`, and off `from` when the two differ.
///
/// Dropping on the root ([`ALL_SHELF`]) re-orders the library's own list rather
/// than a shelf, because "All" IS that list and not a shelf holding a copy of
/// it. `index` is `None` for "append", which is what a drop on empty space
/// means.
///
/// Only ever called while the view is in its manual order: a shelf sorted by
/// title re-sorts on the next render, so a drop there would be undone before the
/// reader saw it land. `library_core::view::LibraryView::drag_reorders` is the
/// question the grid asks before it makes a card draggable.
pub fn move_to_shelf(
    state: AppState,
    book_id: String,
    from: Option<String>,
    to: String,
    index: Option<usize>,
) {
    if to == ALL_SHELF {
        state.library.books.update(|books| {
            let Some(at) = books.iter().position(|b| b.id == book_id) else {
                return;
            };
            let book = books.remove(at);
            // Removing shifts the tail left, so an index past the book's old
            // position is one lower than the reader pointed at.
            let target = match index {
                Some(i) if at < i => i - 1,
                Some(i) => i,
                None => books.len(),
            };
            books.insert(target.min(books.len()), book);
        });
        crate::storage::persist_library(state.library);
        return;
    }

    state.library.shelves.update(|shelves| {
        if let Some(from) = from.as_deref().filter(|id| *id != to)
            && let Some(shelf) = shelves.iter_mut().find(|s| s.id == from)
        {
            shelf::forget(&mut shelf.books, &book_id);
        }
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == to) {
            shelf::place(&mut shelf.books, &book_id, index);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Remove a book from the library — not from the disk it lives on.
///
/// Four things have to happen together, and doing any of them alone leaves the
/// library inconsistent: the row goes, the id comes off every shelf that held it,
/// the folders that placed it take a tombstone so the next rescan stays quiet,
/// and the cover goes with it. A book the app COPIED also loses its store file,
/// because those bytes are the app's and nothing will ever read them again; a
/// linked book loses nothing on disk, which is the whole promise of reading in
/// place.
pub fn remove_book(state: AppState, book_id: String) {
    let found = state
        .library
        .books
        .with_untracked(|books| books.iter().find(|b| b.id == book_id).cloned());
    let Some(book) = found else {
        return;
    };

    let id = book.id.clone();
    let path = book.path().to_string();
    let fingerprint = book.fp;
    let stored = match &book.origin {
        Origin::Stored { store, .. } => Some(store.clone()),
        Origin::Linked { .. } => None,
    };

    state.library.books.update(|books| {
        library_core::book::remove_book(books, &id);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, &id));
    // A removal the reader meant has to survive the file still being on disk,
    // which is what the tombstone is for — and only in the folders that placed
    // it, so removing a hand-added book poisons no watched folder.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, fingerprint));
    state.library.covers.update(|covers| {
        covers.remove(&path);
    });
    if let Some(store) = stored {
        wire::delete_stored(&store);
    }

    // The cover cap only holds if an eviction takes its art with it.
    state.library.books.with_untracked(|books| {
        state
            .library
            .covers
            .update(|covers| prune_covers(books, covers));
    });
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

/// Re-point a book whose address died at a file the reader picks.
///
/// A linked book takes the new address. A stored book does NOT become linked —
/// that would quietly turn "the app keeps its own copy" back into "the app reads
/// your folder again" — so the pick is copied into the store once more, from
/// wherever the file lives now, and the copy is made BEFORE anything is written:
/// a failure to copy leaves the row exactly as it was.
pub fn relink_book(state: AppState, book_id: String, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        let checks = match wire::verify_paths(vec![path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return toast(state, message),
        };
        let Some(fp) = checks.first().and_then(|c| c.fingerprint()) else {
            return toast(
                state,
                "That file is not there any more. Pick the book's current location.".to_string(),
            );
        };
        let origin = state.library.books.with_untracked(|books| {
            books
                .iter()
                .find(|b| b.id == book_id)
                .map(|b| b.origin.clone())
        });
        let Some(origin) = origin else {
            return;
        };

        let store = match origin {
            Origin::Linked { .. } => None,
            Origin::Stored { .. } => {
                let task = format!("relink-{book_id}");
                let requests = [StoreRequest {
                    path: path.clone(),
                    id: book_id.clone(),
                }];
                match wire::store_books(&task, &requests).await {
                    Ok(results) => match results.into_iter().next() {
                        Some(result) if result.is_ok() => Some(result.store),
                        Some(result) => {
                            let message = result
                                .error
                                .unwrap_or_else(|| "Could not copy that file.".to_string());
                            return toast(state, message);
                        }
                        None => return toast(state, "Could not copy that file.".to_string()),
                    },
                    Err(message) => return toast(state, message),
                }
            }
        };

        state.library.books.update(|books| {
            let Some(book) = books.iter_mut().find(|b| b.id == book_id) else {
                return;
            };
            match &mut book.origin {
                Origin::Linked { src } => *src = path.clone(),
                Origin::Stored { src, store: at } => {
                    *src = Some(path.clone());
                    if let Some(store) = store.as_ref() {
                        *at = store.clone();
                    }
                }
            }
            book.fp = fp;
            book.fp_pending = false;
            book.missing = false;
        });
        // Covers are keyed by address, so the old entry now belongs to nobody;
        // the prune drops it and the next open renders the new one.
        state.library.books.with_untracked(|books| {
            state
                .library
                .covers
                .update(|covers| prune_covers(books, covers));
        });
        crate::storage::persist_library(state.library);
        crate::storage::persist_covers(state.library);
    });
}

/// Ask the reader for a file and relink to it.
///
/// The picker is the engine's own (`pdf_engine::api::pick_document`) rather than
/// a second dialog implementation here: it is the same question — "which
/// document?" — with the same filter, and a cancel is the same non-event.
pub fn relink_dialog(state: AppState, book_id: String) {
    spawn_local(async move {
        match pdf_engine::api::pick_document().await {
            Ok(path) => relink_book(state, book_id, path),
            // A cancel is the reader changing their mind, not a failure.
            Err(message) if message == "Open cancelled" => {}
            Err(message) => toast(state, message),
        }
    });
}

fn toast(state: AppState, message: String) {
    state.ui.toast.set(Some(Toast::new(message)));
}
