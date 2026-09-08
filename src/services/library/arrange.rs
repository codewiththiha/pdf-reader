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
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf::{self, Shelf, ALL_SHELF, shelf_add};
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

/// What a removal is allowed to take with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeOpts {
    /// Delete the app's own copy of a stored book. Only ever offered for a book
    /// the app copied; a linked book's bytes belong to the reader and are never
    /// touched whatever this says.
    pub delete_store_copy: bool,
}

impl Default for PurgeOpts {
    /// On, because a copy the app made for a book that is no longer in the library
    /// is a file nothing will ever read again — and the sheet that offers the
    /// switch is the place to say otherwise.
    fn default() -> Self {
        Self {
            delete_store_copy: true,
        }
    }
}

/// Remove a book, and everything the library holds about it.
///
/// Seven things have to happen together, and doing any of them alone leaves
/// something behind that nothing will ever collect: the row (which is the resume
/// point), every shelf membership, the cover, the highlights, the store copy when
/// the app made one, and — in the folders that placed it — a tombstone. The
/// tombstone is the one that is easy to forget and expensive to: without it the
/// file is still on disk and still admitted by the folder's options, so the next
/// focus rescan puts the book straight back.
///
/// Safe while the book is open in the reader. `close_document` and the
/// reading-progress debounce both *update* an entry they find and do nothing when
/// they do not, and `shelf::record` only runs on an open — so a purge never
/// resurrects itself from the document it was purged under.
pub fn purge_book(state: AppState, book_id: &str, opts: PurgeOpts) {
    let found = state
        .library
        .books
        .with_untracked(|books| books.iter().find(|b| b.id == book_id).cloned());
    let Some(book) = found else {
        return;
    };

    // Read the world once, before anything is written: the tombstone needs the
    // folder that placed this book and the shelf it was filed on, and both are
    // about to change.
    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let placed_by = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .map(|f| f.id.clone());
    let home = placed_by
        .as_deref()
        .and_then(|folder_id| folder_shelf_of(&shelves, folder_id, &book.id));
    let entry = Tombstone::of(&book, home, js_sys::Date::now() as u64);
    let path = book.path().to_string();
    let stored_copy = match &book.origin {
        Origin::Stored { store, .. } if opts.delete_store_copy => Some(store.clone()),
        _ => None,
    };

    state.library.books.update(|books| {
        library_core::book::remove_book(books, book_id);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, book_id));
    // Only the folders that placed it: removing a book the reader added by hand
    // must not poison a watched folder that happens to hold the same file, and
    // removing a book one folder placed must not stop a second folder from ever
    // offering it.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    state.library.covers.update(|covers| {
        covers.remove(&path);
    });
    // The highlights are the largest thing the library holds about a book besides
    // its cover, and they are keyed by an address nothing points at any more.
    crate::storage::remove_gloss(&path);
    if let Some(store) = stored_copy {
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

/// The first of one folder's shelves a book is filed on, in shelf order.
///
/// One answer rather than every answer, because a removed book comes back to ONE
/// shelf and a tombstone that listed three would have to choose at restore time
/// with less information than it has now.
fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| {
            s.kind.folder_id() == Some(folder_id) && s.books.iter().any(|m| m == book_id)
        })
        .map(|s| s.id.clone())
}

/// File a book on a second shelf without moving it.
///
/// One book, two memberships, and nothing copied anywhere — a shelf holds ids, so
/// "also show it here" is the cheapest thing in the app and the one that cannot go
/// wrong on disk. The folder's ledger is untouched too: the book stays placed
/// where it was placed, which is what keeps the next rescan quiet about it.
pub fn also_show(state: AppState, book_id: &str, shelf_id: &str) {
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
            shelf_add(shelf, book_id);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Make a shelf the reader owns, and drill into it. Returns its id.
///
/// Named "New shelf" and left there on purpose: a modal that asks for a name
/// before the shelf exists is a modal the reader has to answer to find out what
/// they were asking for, and the breadcrumb's rename is one keystroke away and
/// shows the shelf it is naming.
pub fn new_shelf(state: AppState) -> String {
    let now = js_sys::Date::now() as u64;
    let seq = state
        .library
        .shelves
        .with_untracked(|shelves| shelves.len() as u32);
    let id = library_core::id::new_shelf_id(now, seq);
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf {
            id: made,
            name: "New shelf".to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: Vec::new(),
        });
    });
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Rename a shelf. A blank name is refused rather than stored: a crumb with
/// nothing on it is a crumb the reader cannot click, and a shelf tile with no name
/// is a strip of covers with no way in.
pub fn rename_shelf(state: AppState, shelf_id: &str, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let name = name.to_string();
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
            shelf.name = name;
        }
    });
    crate::storage::persist_library(state.library);
}

/// Take a shelf apart. The books stay in the library — a shelf is a list of ids
/// and never held a byte — and the page steps back out to the root, because the
/// thing it was looking at is gone.
///
/// Only offered for a shelf the reader made. A folder's shelf is derived from the
/// tree, so removing one would be undone by the next file that lands in it, and a
/// control that appears to work and then does not is worse than no control.
pub fn delete_shelf(state: AppState, shelf_id: &str) {
    state.library.shelves.update(|shelves| {
        shelves.retain(|s| s.id != shelf_id);
    });
    state.library.shelf.update(|at| {
        if at == shelf_id {
            *at = ALL_SHELF.to_string();
        }
    });
    crate::storage::persist_library(state.library);
}

/// The shelves a book is on, as `(id, name)` pairs in shelf order. What the
/// folder's restore menu asks in order to tell a book that moved from one that is
/// still where it was filed.
pub fn memberships(state: AppState, book_id: &str) -> Vec<(String, String)> {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.books.iter().any(|m| m == book_id))
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    })
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
