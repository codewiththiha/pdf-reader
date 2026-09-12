//! The removal, and everything the library holds about each removed book:
//! the row, the memberships, the cover, the highlights, the store copy when
//! the app made one, and the tombstone that keeps a watched folder's rescan
//! from putting the book straight back.

use leptos::prelude::*;

use library_core::book::{Book, Row, book_rows, drop_dangling_links, find_row, remove_row};
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf;

use crate::services::library::covers::prune_now;
use crate::services::library as wire;
use crate::state::AppState;
use crate::time::now_ms;

use super::folder_shelf_of;

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

// ---------------------------------------------------------------------------
// The departure: a read-at-place book becomes the library's own copy.
// ---------------------------------------------------------------------------

/// Remove books, and everything the library holds about each of them. One book is
/// a batch of one: the sheet is the only caller and it always holds a list.
///
/// Seven things have to happen together per book, and doing any of them alone
/// leaves something behind that nothing will ever collect: the row (which is the
/// resume point), every shelf membership, the cover, the highlights, the store copy
/// when the app made one, and — in the folders that placed it — a tombstone. The
/// tombstone is the one that is easy to forget and expensive to: without it the file
/// is still on disk and still admitted by the folder's options, so the next focus
/// rescan puts the book straight back. It is per book and per folder for the same
/// reason a batch is not one tombstone: two of a removed ten may have come from
/// different watched folders, and each has to be kept out of its own.
///
/// The cover, the highlights and the store copy go through [`sweep_book`],
/// which is [`sweep_path`]'s guard plus the marks only this row could read: a
/// duplicate the reader chose to keep shares its address — and with it its art
/// and its marks — with the row being removed, and a twin still on the shelf
/// keeps them, while a book of its own takes its own marks with it whatever
/// else is left at the address.
///
/// One persist for the batch rather than one per book. A bulk removal writes the
/// whole blob, and writing it nine times for ten books is nine chances for the
/// reader to close the window mid-way through them.
///
/// Safe while one of the books is open in the reader. `close_document` and the
/// reading-progress debounce both *update* an entry they find and do nothing when
/// they do not, and `shelf::record` only runs on an open — so a purge never
/// resurrects itself from the document it was purged under.
pub fn purge_books(state: AppState, row_ids: &[String], opts: PurgeOpts) {
    let doomed: Vec<Row> = state.library.books.with_untracked(|rows| {
        rows.iter()
            .filter(|r| row_ids.iter().any(|id| id == r.id()))
            .cloned()
            .collect()
    });
    if doomed.is_empty() {
        return;
    }
    for row in &doomed {
        purge_one(state, row, opts);
    }

    // The cover cap only holds if an eviction takes its art with it, and the blob
    // is written once for the batch.
    prune_now(state);
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

/// One book's half of a removal. Reads the world before writing any of it, because
/// the tombstone needs the folder that placed this book and the shelf it was filed
/// on, and both are about to change.
fn purge_one(state: AppState, row: &Row, opts: PurgeOpts) {
    let row_id = row.id();
    // A link is a pointer and nothing else: no address to sweep, no
    // fingerprint to tombstone, no store copy to delete and no highlights to
    // take. Off the list and off every shelf is the whole of its removal — and
    // writing a tombstone for a fingerprint it does not have would keep a file
    // out of a watched folder that never placed it.
    let Some(book) = row.book() else {
        unlist_row(state, row_id);
        return;
    };
    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let placed_by = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .map(|f| f.id.clone());
    let home = placed_by
        .as_deref()
        .and_then(|folder_id| folder_shelf_of(&shelves, folder_id, &book.id));
    let entry = Tombstone::of(book, home, now_ms());
    let was_stored = book.origin.is_stored();

    state.library.books.update(|rows| {
        remove_row(rows, row_id);
        // A link at a book that is gone is a row that renders, is clicked and
        // does nothing, so the pointers at this book go with it. The sweep a
        // load runs would catch them anyway; a removal that left them until
        // then would leave them on screen for the rest of the session.
        drop_dangling_links(rows);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, row_id));
    // Only the folders that placed it: removing a book the reader added by hand
    // must not poison a watched folder that happens to hold the same file, and
    // removing a book one folder placed must not stop a second folder from ever
    // offering it.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    sweep_book(state, book, was_stored && opts.delete_store_copy);
}

/// Drop the side data of an address no remaining row reads from: the
/// highlights, the cached cover and — when the address was the app's own
/// store copy and `delete_store` says the bytes may go — the copy itself.
///
/// The guard is the duplicate rule's other half. Two rows of one file share
/// an address, and with it the gloss and the cover keyed by that address: a
/// sweep that forgot the twin would strip the highlights off a book still on
/// the shelf, and delete the store copy out from under the row reading it.
/// Callers remove their row FIRST, so "remaining" is everybody but the row
/// just gone — which is also what makes a bulk removal of both twins work:
/// the first sweep sees the second row and stays its hand, the second sees
/// nobody and finishes the job.
///
/// "Remaining" is a different list for the two tables, and the difference is
/// a book of its own ([`Book::independent`]): the address's highlights belong
/// to the rows that READ them, which is every row at it except a private one
/// — that keeps its marks under a key of its id, so an address left with only
/// private rows has a mark list nothing will ever paint again, and leaving it
/// is the leak [`crate::storage::remove_gloss`] exists to prevent. The cover
/// is the FILE's art, so any row at the address still earns it.
fn sweep_path(state: AppState, path: &str, delete_store: bool) {
    let (gloss_in_use, path_in_use) = state.library.books.with_untracked(|rows| {
        let mut gloss = false;
        let mut any = false;
        for book in book_rows(rows) {
            if book.path() == path {
                any = true;
                gloss |= !book.independent;
            }
        }
        (gloss, any)
    });
    // The highlights are the largest thing the library holds about a book
    // besides its cover, and they are keyed by an address nothing points at
    // any more.
    if !gloss_in_use {
        crate::storage::remove_gloss(path);
    }
    if path_in_use {
        return;
    }
    state.library.covers.update(|covers| {
        covers.remove(path);
    });
    if delete_store {
        wire::delete_stored(path);
    }
}

/// One removed row's side data: the marks that were its ALONE, and then the
/// address's own sweep.
///
/// A private row's marks are keyed by its id
/// ([`library_core::book::Book::gloss_key`]), which is why removing one takes
/// nothing from its twin — and why [`sweep_path`]'s address guard can never
/// see them. They go with the row that owned them, always, and the address's
/// tables are then swept by their own rule.
fn sweep_book(state: AppState, book: &Book, delete_store: bool) {
    if book.independent {
        crate::storage::remove_gloss(&book.gloss_key());
    }
    sweep_path(state, book.path(), delete_store);
}

/// Take a row off the library's list and off every shelf, and drop the links
/// that pointed at it. The whole of a removal that is NOT a sweep: no tombstone,
/// no cover, no highlights, no store copy.
///
/// One spelling because three callers wanted exactly this and each wrote it out
/// — [`drop_row`], a purge of a link row, and the conflict sheet's link answer —
/// and the half that is easy to forget is the expensive one: a sweep that drops
/// the row but not the pointers at it leaves a row on the shelf that renders, is
/// clicked, and does nothing, for the rest of the session rather than until the
/// next load's sanitize.
pub(crate) fn unlist_row(state: AppState, row_id: &str) {
    state.library.books.update(|rows| {
        remove_row(rows, row_id);
        drop_dangling_links(rows);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, row_id));
}

/// Remove one row, everywhere it is filed, and sweep the side data only it
/// used. Returns the row that went.
///
/// The conflict sheet's removal — a Replace's displaced row and a Merge's
/// dissolving one both go through here — and lighter than [`purge_one`] in
/// exactly one way: no tombstone. The content stays in the library through the
/// row on the other side of the question, so a folder rescan that re-found it
/// would resolve to that row, and a tombstone for a fingerprint the library
/// still holds is noise in the folder's restore menu until the next scan
/// prunes it.
pub(crate) fn drop_row(state: AppState, row_id: &str) -> Option<Row> {
    let row = state
        .library
        .books
        .with_untracked(|rows| find_row(rows, row_id).cloned())?;
    unlist_row(state, row_id);
    if let Some(book) = row.book() {
        sweep_book(state, book, book.origin.is_stored());
    }
    crate::storage::persist_library(state.library);
    Some(row)
}
