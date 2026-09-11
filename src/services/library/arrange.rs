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

use library_core::book::{Book, Origin, Row, book_rows, drop_dangling_links, find_row, remove_row};
use library_core::conflict::Arrival;
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf::{self, Shelf, ALL_SHELF, shelf_add};
use library_core::wire::StoreRequest;

use super::conflict;
use super::covers::prune_now;
use crate::services::library as wire;
use crate::state::AppState;

/// Move books: onto `to` at `index`, and off `from` when the two differ.
///
/// A whole drag in one call, whatever it held. A drag of four books is one action
/// from the reader's side, and the blob is written once for it — the rule
/// [`purge_books`] gives for a bulk removal, for the same reason: a reader who
/// closes the window halfway through a move should find all of it or none of it.
///
/// A library that already holds the very content being dropped is a question
/// rather than a placement — this is the seam where a book used to vanish
/// (the old rule filed the row the library already had, and a shelf that had
/// it skipped the filing): the drag splits through [`conflict::screen`], the
/// clean half lands now and the collisions go to the sheet. A drag of four
/// books with one collision files three and asks about one. The screen is the
/// library's and not the target's, so the twin that asks can be filed on a
/// parent, a child or a sibling of the shelf the hand is over.
///
/// Dropping on the root ([`ALL_SHELF`]) re-orders the library's own list rather
/// than a shelf, because "All" IS that list and not a shelf holding a copy of it.
/// `index` is `None` for "append", which is what a drop on empty space means.
/// The root screens before it reorders, and it is the one level that screens
/// against its OWN list — the unfiled rows — so a book dropped at the root
/// where its own content already lies unfiled is the sheet's question (see
/// `crate::services::library::conflict`) rather than a second silent twin,
/// while a twin filed on a shelf leaves the drop to land as its own row; a row
/// already unfiled is a reorder and never asks.
///
/// `index` counts the level the reader pointed at BEFORE the lift, and the two
/// placements below correct for the books the lift shifts left — a correction one
/// book needs once and four books need together, because the second of them lands
/// where the first one just was.
///
/// Only ever called with an index while the view is in its manual order: a shelf
/// sorted by title re-sorts on the next render, so a drop there would be undone
/// before the reader saw it land. `library_core::view::LibraryView::drag_reorders`
/// is the question the drop asks before it names a position.
pub fn move_many_to_shelf(
    state: AppState,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
) {
    if book_ids.is_empty() {
        return;
    }
    if to == ALL_SHELF {
        // Screened like a shelf: the unfiled list IS the root's member list.
        // The common case — rows already unfiled, reordering among themselves
        // — screens clean by the rule's own reorder arm and reorders exactly
        // as before.
        let (clean, conflicts) = conflict::screen(state, moved_arrivals(state, book_ids, &to, index));
        let clean_ids = clean_move_ids(clean);
        if !clean_ids.is_empty() {
            state
                .library
                .books
                .update(|rows| reorder_root(rows, &clean_ids, index));
            crate::storage::persist_library(state.library);
        }
        conflict::raise(state, conflicts);
        return;
    }

    let (clean, conflicts) =
        conflict::screen(state, moved_arrivals(state, book_ids, &to, index));
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            if let Some(from) = from.as_deref().filter(|id| *id != to)
                && let Some(shelf) = shelves.iter_mut().find(|s| s.id == from)
            {
                for book_id in &book_ids {
                    shelf::forget(&mut shelf.books, book_id);
                }
            }
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == to) {
                place_many(&mut shelf.books, &book_ids, index);
            }
        });
        crate::storage::persist_library(state.library);
    }
    // Raised after the clean half landed: the sheet counts the questions, and
    // a landing that shifted a member list is one the answers resolve against.
    conflict::raise(state, conflicts);
}

/// One arrival per row a hand is moving, each carrying the name the collision
/// is asked about.
///
/// The name is read here rather than by the rule, because the rule is pure and
/// holds no rows: a drag of four books is four arrivals, and a row that went
/// between the lift and the drop is not one of them — an arrival with no row
/// behind it is an arrival with nothing to place.
fn moved_arrivals(
    state: AppState,
    row_ids: &[String],
    to: &str,
    index: Option<usize>,
) -> Vec<Arrival> {
    state.library.books.with_untracked(|rows| {
        row_ids
            .iter()
            .filter_map(|row_id| {
                let row = find_row(rows, row_id)?;
                Some(Arrival::moved(
                    row_id.clone(),
                    row.display_name(),
                    to.to_string(),
                    index,
                ))
            })
            .collect()
    })
}

/// The row ids of a screened clean half — the moves that may land now. The
/// import half of a screen is [`super::import::land_file`]'s business instead;
/// nothing in this module raises one.
fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// Move ONE row onto one level, with no question asked: the silent half of a
/// placement, and what the conflict sheet calls once an answer has been given.
///
/// Off every shelf it was on and onto the one named, at the slot the drop
/// pointed at. The root is the exception the root always was: it has no member
/// list, so a move there is a lift out of every shelf and — when the drop named
/// a slot — a move inside the library's own order, which IS that level's list.
pub fn move_row(state: AppState, row_id: &str, shelf_id: &str, index: Option<usize>) {
    if shelf_id == ALL_SHELF {
        state
            .library
            .shelves
            .update(|shelves| shelf::forget_everywhere(shelves, row_id));
        if index.is_some() {
            state
                .library
                .books
                .update(|rows| reorder_root(rows, &[row_id.to_string()], index));
        }
        crate::storage::persist_library(state.library);
        return;
    }
    state.library.shelves.update(|shelves| {
        shelf::forget_everywhere(shelves, row_id);
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
            shelf::place(&mut shelf.books, row_id, index);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Take books off a shelf without filing them anywhere else.
///
/// What a drop on the root crumb means from inside a shelf: the reader lifted
/// them OUT, and the root is a level rather than a shelf, so there is no member
/// list to move them to. The books stay in the library — a shelf holds ids and
/// never held a byte — and the folder ledger is untouched, so a watched folder
/// that placed one of them still has its fingerprint and will not offer it back
/// on the next rescan.
///
/// Screened first, because the root DOES have a member list to collide with —
/// the unfiled rows the "All" level renders: a book lifted out beside an
/// unfiled twin of its own content is the sheet's question (see
/// [`conflict::screen`]) rather than a second silent row at the top of the
/// library. The clean half comes off the shelf at once; the collisions ask,
/// and their answers land through the conflict module's own mechanics.
pub fn unfile_books(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let (clean, conflicts) =
        conflict::screen(state, moved_arrivals(state, book_ids, ALL_SHELF, None));
    let book_ids = clean_move_ids(clean);
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) else {
            return;
        };
        for book_id in &book_ids {
            moved |= shelf::forget(&mut shelf.books, book_id);
        }
    });
    // A drop that changed nothing writes nothing: a reader who puts a book back
    // where it was has not edited the library, and a save is a chance to close
    // the window mid-write.
    if moved {
        crate::storage::persist_library(state.library);
    }
    conflict::raise(state, conflicts);
}

/// Re-order the library's own list, which IS the "All" level.
///
/// Lifted out and put back in together rather than one at a time: each book's
/// removal shifts the tail left, so moving four in sequence would have the second
/// one's index mean something the first one's already changed.
fn reorder_root(rows: &mut Vec<Row>, row_ids: &[String], index: Option<usize>) {
    let mut lifted: Vec<(usize, Row)> = row_ids
        .iter()
        .filter_map(|row_id| {
            let was = rows.iter().position(|row| row.id() == row_id.as_str())?;
            Some((was, rows[was].clone()))
        })
        .collect();
    if lifted.is_empty() {
        return;
    }
    // Back to front, so the positions counted above are still true when they are
    // removed.
    let mut positions: Vec<usize> = lifted.iter().map(|(was, _)| *was).collect();
    positions.sort_unstable_by_key(|was| std::cmp::Reverse(*was));
    for was in positions {
        rows.remove(was);
    }
    let shift = index.map_or(0, |at| {
        lifted.iter().filter(|(was, _)| *was < at).count()
    });
    // Put back in the order the reader held them, not the order the list did.
    lifted.sort_by_key(|(_, row)| {
        row_ids
            .iter()
            .position(|row_id| row_id.as_str() == row.id())
            .unwrap_or(usize::MAX)
    });
    insert_many(rows, lifted.into_iter().map(|(_, row)| row), index, shift);
}

/// Put `book_ids` on a member list at `index`, taking them off it first.
///
/// [`shelf::place`] for one book and this for a drag: `place` retains and inserts,
/// which is the same two steps, and doing them per book would leave each one's
/// index counting a list the last one had already changed.
fn place_many(members: &mut Vec<String>, book_ids: &[String], index: Option<usize>) {
    let shift = index.map_or(0, |at| {
        book_ids
            .iter()
            .filter(|book_id| {
                members
                    .iter()
                    .position(|member| member.as_str() == book_id.as_str())
                    .is_some_and(|was| was < at)
            })
            .count()
    });
    for book_id in book_ids {
        shelf::forget(members, book_id);
    }
    insert_many(members, book_ids.iter().cloned(), index, shift);
}

/// Insert `items` at `index`, less the `shift` the lift took off the front of it,
/// and in order — each one after the last rather than each one at the same place,
/// which would put them back reversed.
fn insert_many<T>(list: &mut Vec<T>, items: impl Iterator<Item = T>, index: Option<usize>, shift: usize) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
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
        state.library.books.update(|rows| {
            remove_row(rows, row_id);
        });
        state
            .library
            .shelves
            .update(|shelves| shelf::forget_everywhere(shelves, row_id));
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
    let entry = Tombstone::of(book, home, crate::storage::now_ms());
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
pub(crate) fn sweep_path(state: AppState, path: &str, delete_store: bool) {
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
    state.library.books.update(|rows| {
        remove_row(rows, row_id);
        drop_dangling_links(rows);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, row_id));
    if let Some(book) = row.book() {
        sweep_book(state, book, book.origin.is_stored());
    }
    crate::storage::persist_library(state.library);
    Some(row)
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

/// File one book on a second shelf without moving it.
///
/// One book, two memberships, and nothing copied anywhere — a shelf holds ids, so
/// "also show it here" is the cheapest thing in the app and the one that cannot go
/// wrong on disk. The folder's ledger is untouched too: the book stays placed
/// where it was placed, which is what keeps the next rescan quiet about it.
///
/// Unless the library already holds the same CONTENT under another row — a
/// duplicate the reader kept, or the same file filed anywhere else — and then
/// it is [`file_many`]'s question, which is where this delegates.
pub fn also_show(state: AppState, book_id: &str, shelf_id: &str) {
    file_many(state, &[book_id.to_string()], shelf_id);
}

/// Make a shelf the reader owns, at the level they are looking at, and drill
/// into it. Returns its id.
///
/// Named "New shelf" and left there on purpose: a modal that asks for a name
/// before the shelf exists is a modal the reader has to answer to find out what
/// they were asking for, and the breadcrumb's rename is one keystroke away and
/// shows the shelf it is naming.
pub fn new_shelf(state: AppState) -> String {
    let id = create_shelf(state);
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf the reader owns INSIDE `parent`, and drill into it. Returns its
/// id.
///
/// What a folder's own right-click mints: a shelf made from inside a folder is
/// that folder being subdivided, so the parent is the folder that was asked
/// rather than the level the page happens to be on — a shelf the reader made
/// three folders down appears three folders down, whichever level they are
/// standing on. The drill-in is [`new_shelf`]'s, for the reason it gives.
///
/// No `can_nest` question: a shelf with no children yet closes no loop, and a
/// virtual shelf filed inside a folder shelf is a filing the next rescan leaves
/// alone — the scan re-hangs the folder's own rungs and nothing else.
pub fn new_shelf_in(state: AppState, parent: &str) -> String {
    let id = create_shelf_at(state, Some(parent.to_string()));
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf and stay where you are. What a bulk "file onto a new shelf" wants:
/// the reader picked books on one shelf and asked for them to be on another, and
/// navigating them away from the shelf they were looking at is an answer to a
/// question they did not ask.
///
/// Filed at the level the reader is looking at, because a shelf made from inside a
/// folder is a folder being subdivided and one made from the root is a new top
/// level; "All" is not a shelf, so it is the root.
pub fn create_shelf(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    create_shelf_at(state, shelf::level_of_owned(&at))
}

/// The mint both "new shelf" doors share: one id, one empty virtual row at the
/// level `parent` names, and the search tick that lands it on the frame it is
/// made.
fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(crate::storage::now_ms());
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf {
            id: made,
            name: "New shelf".to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: Vec::new(),
            parent,
            manual_parent: false,
        });
    });
    // A belt-and-braces tick for an open search: the folder filter reads the
    // query and the shelves inside one derive, and re-setting the query
    // guarantees both are seen together on the frame the shelf lands — a new
    // shelf under an open search appears at once rather than waiting for the
    // next keystroke to re-run the filter it should already have passed.
    state.library.query.set(state.library.query.get_untracked());
    id
}

/// File one shelf inside another, or back out to the level `parent` names when it
/// is `None`. True when the shelf moved.
///
/// The cycle check is `library_core::shelf::reparent`'s and not the caller's: a
/// folder filed inside itself renders on no level at all and can never be opened
/// again, so the rule has to hold for every caller rather than for every caller
/// that remembered. A refusal writes nothing and persists nothing, which is what
/// lets a drop answer "no" by doing nothing.
///
/// Nesting asks nothing, and that is the rule rather than an oversight: the
/// question a collision asks is about a NAME on a LEVEL, and a nesting writes
/// no membership — the folder keeps its own member list and hangs inside the
/// parent. Nothing arrives on the parent's level, so nothing collides with
/// what is on it, and a book inside a folder is a row the folder's own level
/// asks about when the reader next moves it.
pub fn nest_shelf(state: AppState, folder_id: &str, parent: Option<&str>) -> bool {
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        moved = shelf::reparent(shelves, folder_id, parent);
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
    moved
}

/// File several shelves inside one at once. What a bulk "add to shelf" does with
/// the folders in the set: the books are memberships and the folders are nestings,
/// and one persist covers the batch.
///
/// The folders that actually moved are the ones the persist covers, and no
/// question is asked about any of them: a nesting writes no membership, so
/// nothing arrives on the parent's level for a name to collide with (see
/// [`nest_shelf`]).
pub fn nest_many(state: AppState, folder_ids: &[String], parent: &str) {
    if folder_ids.is_empty() {
        return;
    }
    let mut moved_ids: Vec<String> = Vec::new();
    state.library.shelves.update(|shelves| {
        for folder_id in folder_ids {
            // Each one is asked separately: a batch that contained a folder and
            // one of its own children must file the first and refuse the second,
            // and a single all-or-nothing answer would lose one of the two.
            if shelf::reparent(shelves, folder_id, Some(parent)) {
                moved_ids.push(folder_id.clone());
            }
        }
    });
    if !moved_ids.is_empty() {
        crate::storage::persist_library(state.library);
    }
}

/// Move shelves beside one of their own kind: into the anchor's level, at the
/// anchor's place in it, before or after. What a drag onto a shelf ROW's edge
/// commits — the sibling seam the list layout draws — and a reorder rather than
/// a filing wherever the two shelves already share a level, which is the common
/// case: the reader is not changing the tree, they are changing the order the
/// level renders it in.
///
/// Two steps per shelf because the shelf list IS the render order: `reparent`
/// writes the edge (and refuses the loop the graph would close, or a rung the
/// disk owns), and the splice writes the position — `children_of` filters the
/// list in order, so a moved row that kept its old place in the vec would keep
/// its old place on the page. The anchor's index is re-found after every lift,
/// because a removal above it shifts it, and one persist covers the batch.
pub fn reorder_shelves_to_anchor(state: AppState, ids: &[String], anchor: &str, after: bool) {
    if ids.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let parent = shelves
            .iter()
            .find(|s| s.id == anchor)
            .and_then(|s| s.parent.clone());
        for id in ids {
            if id == anchor || !shelf::reparent(shelves, id, parent.as_deref()) {
                continue;
            }
            // Both positions found before the lift: removing the row first
            // would move the anchor under a hand that had already aimed.
            let (Some(at), Some(mut ai)) = (
                shelves.iter().position(|s| s.id == *id),
                shelves.iter().position(|s| s.id == anchor),
            ) else {
                continue;
            };
            let item = shelves.remove(at);
            if at < ai {
                ai -= 1;
            }
            shelves.insert(if after { ai + 1 } else { ai }, item);
            moved = true;
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
}

/// File several books on one shelf at once.
///
/// Membership only, so the same rule covers a bulk filing as covers a drag: a
/// shelf holds ids, nothing here touches a filesystem, and a book already on the
/// shelf is not moved to the end of it for being named twice. A book whose
/// CONTENT another shelf already holds under another row is the conflict
/// sheet's question rather than a member: the clean half of the batch files now
/// and the collisions ask.
pub fn file_many(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let (clean, conflicts) =
        conflict::screen(state, moved_arrivals(state, book_ids, shelf_id, None));
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) else {
                return;
            };
            for book_id in &book_ids {
                shelf_add(shelf, book_id);
            }
        });
        crate::storage::persist_library(state.library);
    }
    conflict::raise(state, conflicts);
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
/// and never held a byte — and the page steps back out a level, because the thing
/// it was looking at is gone.
///
/// The shelves inside it move up to the level it was on, for the same reason the
/// books stay: a child left pointing at a parent that is gone renders on no level
/// at all, and a reader who removed one folder did not ask to lose the folders
/// filed in it. Stepping out goes to the removed shelf's own parent rather than
/// always to the root, so removing a folder three levels down leaves the reader
/// two levels down and not at the top of the library.
///
/// A folder's shelf is removable too, and the receipt says what that means: the
/// shelf comes off the list and the folder keeps watching, so it returns if the
/// folder ever places a book in it again. That is the honest reading of "watched"
/// rather than a control that appears to work and then does not — the shelf map's
/// pointer is cut here, so a returning shelf is a new shelf, not a ghost.
pub fn delete_shelf(state: AppState, shelf_id: &str) {
    let was_inside = state.library.shelf.get_untracked() == shelf_id;
    let stepped_out = state
        .library
        .shelves
        .with_untracked(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == shelf_id)
                .and_then(|gone| gone.parent.clone())
        })
        .unwrap_or_else(|| ALL_SHELF.to_string());
    // The folder tree's own pointer at this shelf, cut as well. Left in place,
    // a watched folder that places a book here again would file it onto a shelf
    // that no longer exists — a ghost row the reader can neither see nor remove,
    // and the one way a removal could lose a book rather than a shelf.
    let detached = state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .and_then(|s| s.kind.folder_id().map(str::to_string))
    });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
    });
    if let Some(folder_id) = detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folders.iter_mut().find(|f| f.id == folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
            }
        });
    }
    if was_inside {
        state.library.shelf.set(stepped_out);
    }
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
            Err(message) => return state.toast(message),
        };
        let Some(fp) = checks.first().and_then(|c| c.fingerprint()) else {
            return state.toast(
                "That file is not there any more. Pick the book's current location.",
            );
        };
        let origin = state.library.books.with_untracked(|rows| {
            library_core::book::find_by_id(rows, &book_id).map(|b| b.origin.clone())
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
                            return state.toast(message);
                        }
                        None => return state.toast("Could not copy that file.".to_string()),
                    },
                    Err(message) => return state.toast(message),
                }
            }
        };

        state.library.books.update(|rows| {
            let Some(book) = library_core::book::book_rows_mut(rows).find(|b| b.id == book_id)
            else {
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
        prune_now(state);
        crate::storage::persist_library(state.library);
        crate::storage::persist_covers(state.library);
        // The old address's cover belongs to nobody now, and the new one has
        // never been rendered: queue it rather than waiting for an open.
        super::covers::backfill_missing(state);
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
            Err(message) if reader_core::filename::is_cancelled(&message) => {}
            Err(message) => state.toast(message),
        }
    });
}
