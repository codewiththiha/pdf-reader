//! The moves a reader makes by hand: a drag between shelves, a removal, a
//! relink.
//!
//! One rule covers the membership half of all three — **a move never touches a
//! file the reader owns.** A shelf holds book ids, so a drag edits a list of
//! ids, and the OS file a read-in-place book points at is never renamed, moved
//! or deleted from here. The only byte this module deletes belongs to a book
//! the app itself copied into its store, and that goes through the shell's
//! contained `delete_stored`.
//!
//! A move DOES copy one thing, and only on a departure: a read-at-place book
//! leaving the ground that made it becomes the library's own stored copy on
//! the way out ([`convert_to_stored`]), because a book no folder answers for
//! has to be a book the library holds outright — its bytes its own, its
//! identity the copy's own fingerprint, and the ORIGINAL fingerprint free for
//! the folder's log to keep. The ground is the rung the folder's own tree
//! names for the file's address and not the folder's shelf tree as a whole, so
//! a drag from one rung of a watched folder to another departs as well; only a
//! re-order on the book's own rung, and every move of a book that is already
//! stored, stays a membership edit.
//!
//! The rule is also what makes the ledger's hardest row true without anybody
//! having to remember it: dragging a book off a watched folder's shelf leaves
//! its fingerprint in that folder's `placed` set — and a departure that
//! converted leaves a moved-out log beside it — so the next rescan skips the
//! file instead of filing it straight back where the reader just moved it
//! from, and a later import of it knows what it is bringing home.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{
    Book, Origin, Row, book_rows, drop_dangling_links, find_book_mut, find_row, remove_row,
};
use library_core::conflict::{Arrival, same_name};
use library_core::folder::{self as folder_ops, Tombstone};
use library_core::ledger::tombstone;
use library_core::shelf::{self, Shelf, ALL_SHELF, shelf_add};

use super::conflict;
use super::covers::prune_now;
use crate::services::library as wire;
use crate::time::now_ms;
use crate::state::{AppState, Toast};

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
    seat_many(state, book_ids, from, to, index, &[]);
}

/// [`move_many_to_shelf`], knowing which rows THIS gesture turned into copies.
///
/// The public entry has converted nothing yet, so it passes an empty list and
/// every stored book it lands can bind a folder's moved-out log as a return.
/// The departure gate's retry passes the rows it just copied, and those land
/// without binding: the log they wrote is the one a bind would find, and a
/// departure is not a return — see [`convert_departures`].
fn seat_many(
    state: AppState,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
    departed: &[String],
) {
    if book_ids.is_empty() {
        return;
    }
    // The read-at-place departure gate: a linked book of an in-place folder
    // leaving its level becomes the library's own stored copy FIRST, and the
    // whole move then runs again over the converted rows — one flow and one
    // ordering, and every screen, sheet and shelf write downstream sees the
    // books as what they are about to be. A copy that fails costs that book
    // its move and nothing else: it stays where it was, linked, and the toast
    // says so.
    if to != ALL_SHELF && from.as_deref().is_some_and(|f| f != to) {
        let (lifted_from, landed_on) = (from.clone(), to.clone());
        if convert_departures(state, book_ids, &to, move |rest, gone| {
            seat_many(state, &rest, lifted_from, landed_on, index, &gone)
        }) {
            return;
        }
    }
    if to == ALL_SHELF {
        // Screened like a shelf: the unfiled list IS the root's member list.
        // The common case — rows already unfiled, reordering among themselves
        // — screens clean by the rule's own reorder arm and reorders exactly
        // as before.
        let (clean, conflicts) = conflict::screen(
            state,
            moved_arrivals(state, book_ids, &to, index, from.as_deref()),
        );
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

    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, &to, index, from.as_deref()),
    );
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            if let Some(from) = from.as_deref().filter(|id| *id != to)
                && let Some(shelf) = shelf::find_mut(shelves, from)
            {
                for book_id in &book_ids {
                    shelf::forget(&mut shelf.books, book_id);
                }
            }
            if let Some(shelf) = shelf::find_mut(shelves, &to) {
                place_many(&mut shelf.books, &book_ids, index);
            }
        });
        crate::storage::persist_library(state.library);
        // A stored book landing back on a shelf of the folder it left is a
        // return, and the folder's moved-out log records it — unless this
        // gesture is the one that made the copy, whose landing would bind the
        // very log it just wrote.
        for book_id in &book_ids {
            if !departed.contains(book_id) {
                bind_returned(state, book_id, &to);
            }
        }
    }
    // Raised after the clean half landed: the sheet counts the questions, and
    // a landing that shifted a member list is one the answers resolve against.
    conflict::raise(state, conflicts);
}

/// One arrival per row a hand is moving, each carrying the name the collision
/// is asked about and the level the hand lifted off.
///
/// The name is read here rather than by the rule, because the rule is pure and
/// holds no rows: a drag of four books is four arrivals, and a row that went
/// between the lift and the drop is not one of them — an arrival with no row
/// behind it is an arrival with nothing to place.
///
/// `from` is the departure the answers need: a merge that did not know where
/// the row came from would file the survivor back on that shelf, and the book
/// the reader just moved away would still be sitting where they moved it from.
/// A filing passes none, because a second membership leaves every level as it
/// was.
fn moved_arrivals(
    state: AppState,
    row_ids: &[String],
    to: &str,
    index: Option<usize>,
    from: Option<&str>,
) -> Vec<Arrival> {
    state.library.books.with_untracked(|rows| {
        row_ids
            .iter()
            .filter_map(|row_id| {
                let row = find_row(rows, row_id)?;
                let arrival = Arrival::moved(
                    row_id.clone(),
                    row.display_name(),
                    to.to_string(),
                    index,
                );
                Some(match from {
                    Some(from) => arrival.leaving(from),
                    None => arrival,
                })
            })
            .collect()
    })
}

/// The row ids of a screened clean half — the moves that may land now. The
/// import half of a screen is [`super::import::land_stored_copy`]'s business
/// instead; nothing in this module raises one.
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
///
/// `departed` says the row became the library's own copy in THIS gesture — the
/// caller converted it, or the gate below did and is running the move again.
/// Such a row lands without binding a folder's moved-out log to itself: the log
/// it would bind is the one its own departure just wrote, and a departure is not
/// a return. See [`convert_departures`].
pub fn move_row(
    state: AppState,
    row_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    departed: bool,
) {
    // The departure gate, for the one-row form: convert first, then run the
    // move again over the stored row, so the shelf writes below are the whole
    // of what happens and happen in one order.
    {
        let (row, landed_on) = (row_id.to_string(), shelf_id.to_string());
        if convert_departures(
            state,
            std::slice::from_ref(&row),
            shelf_id,
            move |rest, gone| {
                if let Some(one) = rest.into_iter().next() {
                    move_row(state, &one, &landed_on, index, gone.contains(&one));
                }
            },
        ) {
            return;
        }
    }
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
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
            shelf::place(&mut shelf.books, row_id, index);
        }
    });
    crate::storage::persist_library(state.library);
    if !departed {
        bind_returned(state, row_id, shelf_id);
    }
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
    // A lift OUT of a folder's shelf is a departure like any other move: a
    // read-at-place book becomes the library's own copy on the way out, and
    // the lift then runs again over the stored rows. The rows it converted are
    // of no interest here: a lift out files the book on no shelf, and only a
    // landing on a shelf of the folder it left can read as a return.
    {
        let lifted_from = shelf_id.to_string();
        if convert_departures(state, book_ids, ALL_SHELF, move |rest, _| {
            unfile_books(state, &rest, &lifted_from)
        }) {
            return;
        }
    }
    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, ALL_SHELF, None, Some(shelf_id)),
    );
    let book_ids = clean_move_ids(clean);
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
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

/// Copy every read-at-place book about to leave its folder's ground, then run
/// the move again over the survivors. Answers whether a conversion started, which
/// is the caller's whole question: it did, so the move has not happened yet and
/// the caller returns.
///
/// One spelling for the three moves that owe a departure — a drag between
/// shelves, a lift out to the root, and the one-row form the conflict sheet rides
/// — because the ORDER is the whole of the rule. The copy is made and the row is
/// converted BEFORE any shelf write happens, so every screen, sheet and
/// membership edit downstream sees the books as what they are about to be rather
/// than as what they were when the hand lifted. Three copies of this were three
/// places to get that order wrong.
///
/// A copy that fails costs that book its move and nothing else: it stays where it
/// was, linked, the toast says so, and the other books in the same drag still go.
/// `retry` runs the move over the survivors and is called from the spawned task,
/// which is what lets the caller return at once and keep its own shape.
///
/// `retry` takes the survivors AND the rows this gate turned into copies, because
/// the landing owes them one exception: a departure writes a moved-out log, and
/// the copy then lands — often on another shelf of the very folder it left, which
/// is where a reader re-arranging a watched tree puts it. [`bind_returned`] reads
/// a stored book landing on a shelf of the folder it left as the book coming
/// HOME, and a log bound to a row is one a later import answers by lighting that
/// row up instead of bringing the linked book back. One gesture cannot be both
/// the departure and the return, so the rows this gate converted are named to the
/// landing and the bind stands aside for them; a drag of the same row back on a
/// LATER gesture is a return and binds as it always did.
fn convert_departures(
    state: AppState,
    ids: &[String],
    to: &str,
    retry: impl FnOnce(Vec<String>, Vec<String>) + 'static,
) -> bool {
    if !tauri_bridge::has_tauri() {
        return false;
    }
    let converting: Vec<String> = ids
        .iter()
        .filter(|id| converts_on_move_to(state, id, to))
        .cloned()
        .collect();
    if converting.is_empty() {
        return false;
    }
    let all: Vec<String> = ids.to_vec();
    spawn_local(async move {
        let mut failed: Vec<String> = Vec::new();
        for id in &converting {
            if let Err(message) = convert_to_stored(state, id).await {
                failed.push(id.clone());
                toast(state, message);
            }
        }
        super::covers::backfill_missing(state);
        let rest: Vec<String> = all.into_iter().filter(|id| !failed.contains(id)).collect();
        if !rest.is_empty() {
            // A copy that failed left the row linked and logged nothing, so it
            // departs nothing either.
            let departed: Vec<String> = converting
                .into_iter()
                .filter(|id| !failed.contains(id))
                .collect();
            retry(rest, departed);
        }
    });
    true
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

// ---------------------------------------------------------------------------
// The departure: a read-at-place book becomes the library's own copy.
// ---------------------------------------------------------------------------

/// Whether moving this row to this level is a departure that owes a copy: a
/// read-at-place book an in-place folder placed, leaving the rung that folder's
/// own tree names for the file's address.
///
/// The three negatives are as load-bearing as the positive. A STORED book is
/// already the library's own and simply moves. A book no in-place folder placed
/// — a loose file the reader dropped, a book of a copying folder — has no ledger
/// waiting on its fingerprint and moves as a membership. And a re-order on the
/// book's OWN rung is the folder's business: the file is still standing on the
/// ground that made it, so no copy is made and no log is written.
///
/// Everywhere else is a departure, **including another rung of the very folder
/// that placed the book.** What ties a read-at-place book to a folder is the
/// ground its file stands on, not the folder's shelf tree: a book dragged up
/// from `Sci-Fi/` to `Fiction/` is no longer where the folder's ledger says it
/// is, so it becomes the library's own copy on the way out and the ORIGINAL
/// fingerprint goes free for the log to keep. Reading the tie as the tree
/// instead is what let a moved book go on wearing the address — the next import
/// of that same file then found a living row at it, asked the reader to choose
/// between a collision and a highlight, and pointed at the row they had dragged
/// away rather than bringing the file home to the rung it belongs on.
pub(crate) fn converts_on_move_to(state: AppState, row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .filter(|book| matches!(book.origin, Origin::Linked { .. }))
            .map(|book| (book.fp, book.path().to_string()))
    }) else {
        return false;
    };
    // One entry per in-place folder whose ledger answers for this fingerprint:
    // the rung that folder names for the address, or `None` when it names none
    // — a rung the reader has deleted since the walk that placed the file, whose
    // book has left the ground all the same. An EMPTY list is therefore the only
    // thing the length says: no ledger is waiting, so no departure is owed.
    let rungs: Vec<Option<String>> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|f| f.opts.in_place && f.placed.contains(&fp))
            .map(|f| f.rungs_for(&path).0.map(str::to_string))
            .collect()
    });
    if rungs.is_empty() {
        return false;
    }
    // The root is nobody's rung: "All" is the library's own list rather than a
    // shelf, so no folder's map can name it and a book that lands there has left
    // every ground. Spelled out because it is the departure readers make most,
    // and because reading it off the list alone would leave the answer depending
    // on a map never holding a level that is not a shelf.
    to == ALL_SHELF || !rungs.iter().any(|rung| rung.as_deref() == Some(to))
}

/// Make a read-at-place book the library's own stored copy: the departure
/// half of a move, and the only byte a hand-move ever writes.
///
/// Why a move copies: the book is leaving the ground that made it. Inside its
/// folder's tree the row IS the OS file — the ledger answers for it, a rescan
/// keeps it in place, a removal logs it. On a shelf of its own choosing it can
/// be none of those things without becoming a book the library holds outright,
/// so it becomes one: the bytes go into the store, the row's identity becomes
/// the copy's own measurement, and — the point of the whole rule — the
/// ORIGINAL fingerprint is left free. The folder takes a moved-out log for it,
/// which keeps every rescan quiet, keeps the restore menu honest (the book is
/// not gone), and lets a later import of the OS file bring the linked book
/// back beside the copy that left: two books of one content, each with one
/// address, no twins.
///
/// Everything the reader put into the row travels with it. The visible name
/// moves into `title`, because the store file is named after the row's id and
/// a shelf reading "b1c2d3" is a shelf that renamed the book. The resume point
/// and the format ride the row. The highlights move their key from the old
/// address to the copy's — moved outright when no twin still reads the old
/// address, copied when one does. A measurement of the fresh copy that fails
/// leaves the old fingerprint flagged pending rather than blocking the move:
/// the startup sweep re-measures the store path and finishes the job.
pub(crate) async fn convert_to_stored(state: AppState, row_id: &str) -> Result<(), String> {
    let Some(book) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .cloned()
    }) else {
        return Err("That book is no longer in the library.".to_string());
    };
    if book.origin.is_stored() {
        // Already the library's own: a second departure of one book must not
        // make a second copy of it.
        return Ok(());
    }
    let path = book.path().to_string();
    let from_key = book.gloss_key();
    let store = wire::copy_one_to_store(&format!("move-{row_id}"), &path, row_id).await?;
    let measured = wire::verify_paths(vec![store.clone()])
        .await
        .ok()
        .and_then(|checks| checks.into_iter().next())
        .and_then(|check| check.fingerprint());

    // The log first, while the row still sits on the folder's shelf: the
    // tombstone records the shelf it was filed on, and that is a fact about
    // the world before the move, not after it.
    write_moved_stones(state, &book, None);

    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.become_stored(&path, store.clone(), measured);
        }
    });
    let to_key = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .map(Book::gloss_key)
    });
    if let Some(to) = to_key
        && to != from_key
    {
        migrate_gloss(state, &from_key, &to, &path);
    }
    // The old address's cover belongs to the file the row no longer reads, and
    // the copy has never been rendered: prune one, queue the other.
    prune_now(state);
    crate::storage::persist_library(state.library);
    Ok(())
}

/// The moved-out log for a read-at-place book: every folder that placed its
/// fingerprint records that the book left as the library's own copy rather
/// than died.
///
/// `returned_row` names the row the file is represented by, for the two
/// answers that dissolve a linked row into a book the library already holds —
/// a merge into its stored copy and a link at one. A conversion leaves it
/// `None`: nothing represents the file yet, and an import of it is owed a real
/// linked book rather than a highlight.
pub(crate) fn write_moved_stones(state: AppState, book: &Book, returned_row: Option<&str>) {
    let home = {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        folders
            .iter()
            .find(|f| f.placed.contains(&book.fp))
            .and_then(|f| folder_shelf_of(&shelves, &f.id, &book.id))
    };
    // The removal's own constructor, with the two facts a departure adds: this
    // book left as the library's copy rather than died, and — when one answer
    // named it — the row the file is represented by from now on. Spelling all
    // eight fields here instead would be a second place a new `Tombstone` field
    // has to be remembered, and the removal's receipt already owns the first six.
    let entry = Tombstone {
        moved: true,
        returned_row: returned_row.map(str::to_string),
        ..Tombstone::of(book, home, now_ms())
    };
    // `ledger::tombstone` writes it only into the folders that placed the
    // fingerprint and do not already hold a log for it — the removal's own
    // rule, and the right one here.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    crate::storage::persist_library(state.library);
}

/// The highlights follow the row's address: a conversion changes the address,
/// and marks left under the old key are marks nothing paints again. Moved
/// outright when no remaining row reads the old address, copied when a shared
/// twin still does — a twin's key IS the address, and the address is still
/// its. A private row's key carries its id, so its list is always a move.
///
/// Crate-visible because the import's mode switch converts a whole shelf of
/// books at once and every flipped row owes its marks the same move.
pub(crate) fn migrate_gloss(state: AppState, from_key: &str, to_key: &str, address: &str) {
    let shared = from_key == address
        && state
            .library
            .books
            .with_untracked(|rows| book_rows(rows).any(|b| b.path() == address));
    let Some(marks) = crate::storage::load_gloss().remove(from_key) else {
        return;
    };
    if marks.is_empty() {
        return;
    }
    crate::storage::persist_gloss(to_key, &marks);
    if !shared {
        crate::storage::remove_gloss(from_key);
    }
}

/// A stored book landing on a shelf of the folder it once left is a return:
/// the folder's moved-out log binds itself to the row, and from then on an
/// import of the OS file highlights THIS row instead of minting a linked
/// neighbour beside the copy that came home.
///
/// The bind is by NAME, which is the whole of the condition: the log remembers
/// the name the shelf showed, and a row wearing that exact name is the book
/// the reader moved back. A row renamed since the move binds nothing — the
/// folder does not recognise it, the log stays unbound, and a later import of
/// the file simply brings the linked book back and lights it up, which is the
/// honest answer for a name the folder has never seen.
///
/// Callers that seat a row THIS gesture turned into a copy do not reach here at
/// all. A departure writes a log and then lands, and a reader re-arranging a
/// watched tree lands it on another shelf of the same folder — which is the
/// shape of a return without being one, and a bind there would spend the log on
/// the row that just left, so the file could never come home again.
/// [`convert_departures`] names those rows to the landing; [`file_many`] has
/// none to name, because a second membership departs nothing.
fn bind_returned(state: AppState, row_id: &str, shelf_id: &str) {
    if shelf_id == ALL_SHELF {
        return;
    }
    let Some(name) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .filter(|row| row.book().is_some_and(|b| b.origin.is_stored()))
            .map(|row| row.display_name())
    }) else {
        return;
    };
    let Some(folder_id) = state.library.shelf_folder_id(shelf_id) else {
        return;
    };
    let mut bound = false;
    state.library.folders.update(|folders| {
        let Some(folder) = folder_ops::find_mut(folders, &folder_id) else {
            return;
        };
        if let Some(entry) = folder
            .ignored
            .iter_mut()
            .find(|entry| entry.moved && same_name(&entry.label(), &name))
        {
            if entry.returned_row.as_deref() != Some(row_id) {
                entry.returned_row = Some(row_id.to_string());
                bound = true;
            }
        }
    });
    if bound {
        crate::storage::persist_library(state.library);
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

/// The first of one folder's shelves a book is filed on, in shelf order.
///
/// One answer rather than every answer, because a removed book comes back to ONE
/// shelf and a tombstone that listed three would have to choose at restore time
/// with less information than it has now.
fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|s| s.kind.folder_id() == Some(folder_id))
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

/// Make a shelf the reader owns, and drill into it. Returns its id.
///
/// `parent` is where the shelf hangs: `None` is the level the page is on, and
/// `Some` is a shelf the reader named — what a folder's own right-click mints,
/// because a shelf made from inside a folder is that folder being subdivided,
/// so the parent is the folder that was asked rather than the level the page
/// happens to be on. A shelf the reader made three folders down appears three
/// folders down, whichever level they are standing on.
///
/// Named "New shelf" and left there on purpose: a modal that asks for a name
/// before the shelf exists is a modal the reader has to answer to find out what
/// they were asking for, and the breadcrumb's rename is one keystroke away and
/// shows the shelf it is naming.
///
/// No `can_nest` question: a shelf with no children yet closes no loop, and a
/// virtual shelf filed inside a folder shelf is a filing the next rescan leaves
/// alone — the scan re-hangs the folder's own rungs and nothing else.
pub fn create_shelf_and_enter(state: AppState, parent: Option<&str>) -> String {
    let id = match parent {
        Some(parent) => create_shelf_at(state, Some(parent.to_string())),
        None => create_shelf_here(state),
    };
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf at the level the reader is looking at, and stay where you are.
/// What a bulk "file onto a new shelf" wants: the reader picked books on one
/// shelf and asked for them to be on another, and navigating them away from
/// the shelf they were looking at is an answer to a question they did not ask.
///
/// Filed at the level the reader is looking at, because a shelf made from inside a
/// folder is a folder being subdivided and one made from the root is a new top
/// level; "All" is not a shelf, so it is the root.
pub fn create_shelf_here(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

/// The mint both "new shelf" doors share: one id, one empty virtual row at the
/// level `parent` names, and the search tick that lands it on the frame it is
/// made.
fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(now_ms());
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
        let parent = shelf::find(shelves, anchor).and_then(|s| s.parent.clone());
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
    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, shelf_id, None, None),
    );
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
                return;
            };
            for book_id in &book_ids {
                shelf_add(shelf, book_id);
            }
        });
        crate::storage::persist_library(state.library);
        // A second membership is not a departure, so nothing converts here —
        // but a stored book shown again on a shelf of the folder it left is a
        // return all the same, and the folder's log records it.
        for book_id in &book_ids {
            bind_returned(state, book_id, shelf_id);
        }
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
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
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
    // One read of the shelf list answers both facts about the shelf that is
    // going: the level to step out to, and — the folder tree's own pointer at
    // this shelf, cut as well — which watched folder filed onto it. Left in
    // place, a folder that places a book here again would file it onto a shelf
    // that no longer exists: a ghost row the reader can neither see nor
    // remove, and the one way a removal could lose a book rather than a shelf.
    let (stepped_out, detached) =
        state
            .library
            .shelves
            .with_untracked(|shelves| {
                shelf::find(shelves, shelf_id)
                    .map_or((ALL_SHELF.to_string(), None), |gone| {
                        (
                            gone.parent
                                .clone()
                                .unwrap_or_else(|| ALL_SHELF.to_string()),
                            gone.kind.folder_id().map(str::to_string),
                        )
                    })
            });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
    });
    if let Some(folder_id) = detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folder_ops::find_mut(folders, &folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
            }
        });
    }
    // A link may point AT a shelf — the pointer a merged import leaves behind —
    // and a pointer at nothing is a row that renders, is clicked and does
    // nothing: the shelf links go with the shelf, the way the book links go
    // with a book.
    let shelves_now = state.library.shelves.get_untracked();
    state.library.books.update(|rows| {
        library_core::book::drop_dead_shelf_links(rows, &shelves_now);
    });
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
        shelf::containing(shelves, book_id)
            .into_iter()
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
fn relink_book(state: AppState, book_id: String, path: String) {
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
                match wire::copy_one_to_store(&task, &path, &book_id).await {
                    Ok(store) => Some(store),
                    Err(message) => return toast(state, message),
                }
            }
        };

        state.library.books.update(|rows| {
            let Some(book) = find_book_mut(rows, &book_id) else {
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
            book.heal(fp);
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
            Err(message) if message == "Open cancelled" => {}
            Err(message) => toast(state, message),
        }
    });
}

pub(crate) fn toast(state: AppState, message: String) {
    state.ui.toast.set(Some(Toast::new(message)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    use library_core::book::{Book, Fingerprint, Origin};
    use library_core::folder::{FolderOpts, WatchedFolder};
    use library_core::shelf::ShelfKind;
    use reader_core::format::Format;

    /// A linked row. Markdown rather than PDF so nothing that reads these lists
    /// ever asks the cover queue to render one — a host test has no engine.
    fn row(id: &str) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            Fingerprint {
                size: 1,
                mtime_ms: 1,
                head_hash: 1,
            },
            Format::Markdown,
            Origin::Linked {
                src: format!("/books/{id}.md"),
            },
            0,
        ))
    }

    fn list() -> Vec<Row> {
        vec![row("a"), row("b"), row("c"), row("d")]
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.id()).collect()
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // -----------------------------------------------------------------------
    // reorder_root — "All" IS the library's own list, so a drop on the root
    // crumb re-orders rows rather than filing them anywhere.
    // -----------------------------------------------------------------------

    #[test]
    fn a_drop_on_the_root_puts_one_row_where_the_reader_pointed() {
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["d"]), Some(1));
        assert_eq!(ids(&rows), vec!["a", "d", "b", "c"]);
    }

    #[test]
    fn the_index_counts_the_list_as_it_was_before_the_lift() {
        // The whole reason `insert_many` takes a `shift`. "a" and "b" sat at 0
        // and 1, so lifting them moves "d" from index 3 to index 1 — and the
        // reader pointed at the slot "d" occupied while they were holding the
        // two. That is the slot they land in, not two further down the list the
        // lift just shortened.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["a", "b"]), Some(3));
        assert_eq!(ids(&rows), vec!["c", "a", "b", "d"]);
    }

    #[test]
    fn a_drop_past_the_end_appends() {
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["a"]), Some(99));
        assert_eq!(ids(&rows), vec!["b", "c", "d", "a"]);
    }

    #[test]
    fn an_append_keeps_the_payload_s_order_not_the_list_s() {
        // A set has no order, so the payload is sorted into the level's own
        // order on the way out — and the payload's order IS the reader's, which
        // is why the sort is by position in `row_ids` and not by the position
        // each row used to hold. Putting them back in the list's order would be
        // a drop that quietly shuffled the hand.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["c", "a"]), None);
        assert_eq!(ids(&rows), vec!["b", "d", "c", "a"]);
    }

    #[test]
    fn a_row_the_list_does_not_hold_is_not_invented() {
        // A drag can outlive a row: a focus rescan or another surface's removal
        // can take it between the lift and the drop. A hole in the grid would be
        // worse than an id quietly dropped.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["gone", "b"]), Some(0));
        assert_eq!(ids(&rows), vec!["b", "a", "c", "d"]);
    }

    #[test]
    fn a_link_is_reordered_by_its_own_id_like_any_other_row() {
        // "All" holds links as well as books, and a drag of one is a question
        // about a position rather than about content.
        let mut rows = vec![
            row("a"),
            Row::link("l1".into(), "Dune".into(), "a".into(), 1),
            row("b"),
        ];
        reorder_root(&mut rows, &owned(&["l1"]), Some(0));
        assert_eq!(ids(&rows), vec!["l1", "a", "b"]);
    }

    #[test]
    fn an_empty_set_leaves_the_list_alone() {
        let mut rows = list();
        reorder_root(&mut rows, &[], Some(0));
        assert_eq!(ids(&rows), vec!["a", "b", "c", "d"]);
    }

    // -----------------------------------------------------------------------
    // place_many — a shelf's member list, which is ids and nothing else.
    // -----------------------------------------------------------------------

    #[test]
    fn a_book_already_on_the_shelf_is_moved_not_duplicated() {
        let mut members = owned(&["a", "b", "c"]);
        place_many(&mut members, &owned(&["a"]), Some(2));
        assert_eq!(
            members,
            vec!["b", "a", "c"],
            "one membership, in the slot the drop named"
        );
    }

    #[test]
    fn the_shift_is_counted_per_book_rather_than_for_the_batch() {
        // "a" and "c" were at 0 and 2, both below the drop's index 3, so the
        // lift takes two off it. "b" was not on the shelf at all and adds
        // nothing to the count — counting the batch instead of the members
        // would have landed the three one slot early.
        let mut members = owned(&["a", "x", "c", "y"]);
        place_many(&mut members, &owned(&["a", "b", "c"]), Some(3));
        assert_eq!(members, vec!["x", "a", "b", "c", "y"]);
    }

    #[test]
    fn filing_with_no_index_appends_in_order() {
        let mut members = owned(&["a"]);
        place_many(&mut members, &owned(&["b", "c"]), None);
        assert_eq!(members, vec!["a", "b", "c"]);
    }

    // -----------------------------------------------------------------------
    // insert_many — the step both of the above land on.
    // -----------------------------------------------------------------------

    #[test]
    fn each_item_lands_after_the_last_rather_than_all_at_one_place() {
        let mut list: Vec<&str> = vec!["x", "y"];
        insert_many(&mut list, ["a", "b", "c"].into_iter(), Some(1), 0);
        assert_eq!(list, vec!["x", "a", "b", "c", "y"], "not reversed");
    }

    #[test]
    fn an_index_past_the_end_clamps_per_item() {
        let mut list: Vec<&str> = vec!["x"];
        insert_many(&mut list, ["a", "b"].into_iter(), Some(99), 0);
        assert_eq!(list, vec!["x", "a", "b"]);
    }

    #[test]
    fn a_shift_larger_than_the_index_lands_at_the_front() {
        let mut list: Vec<&str> = vec!["x", "y"];
        insert_many(&mut list, ["a"].into_iter(), Some(1), 4);
        assert_eq!(list, vec!["a", "x", "y"]);
    }

    #[test]
    fn a_removal_deletes_the_app_s_own_copy_by_default() {
        // A copy the app made for a book that is no longer in the library is a
        // file nothing will ever read again; the sheet is where a reader says
        // otherwise, and it is the only place that does.
        assert!(PurgeOpts::default().delete_store_copy);
    }

    // -----------------------------------------------------------------------
    // converts_on_move_to — the departure, and the ground it measures against.
    // -----------------------------------------------------------------------

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn linked_at(id: &str, path: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked {
                src: path.to_string(),
            },
            0,
        ))
    }

    fn stored_at(id: &str, src: &str, store: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Stored {
                src: Some(src.to_string()),
                store: store.to_string(),
            },
            0,
        ))
    }

    /// `/books` cut into `Fiction` cut into `Fiction/SciFi`, read in place and
    /// holding fingerprint `n` — the three-level shelf the departure rule was
    /// wrong about.
    fn nested(n: u32) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(n)]),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                (String::new(), "shelf1".to_string()),
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            scanned_ms: 0,
        }
    }

    /// The reported shape, written into a fresh state: one read-in-place
    /// folder, three rungs, and the linked book the deepest one placed.
    ///
    /// Takes the state rather than making one because the `Owner` a signal
    /// needs has to outlive the write, and a helper that minted its own would
    /// drop it on the way out.
    fn set_nested(state: AppState) {
        state.library.folders.set(vec![nested(7)]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
    }

    #[test]
    fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // shelf3 is the rung the folder's own tree names for this address, so a
        // drag UP to shelf2 leaves the ground even though shelf2 is a rung of
        // the very folder that placed the book. Reading the tie as the folder's
        // shelf tree instead — "any shelf this folder owns" — left the row
        // linked at an address it had been dragged off, and the next import of
        // that file found a living row there, asked the reader to choose between
        // a collision and a highlight, and lit up the row that had moved rather
        // than bringing the file home to the rung it belongs on.
        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(converts_on_move_to(state, "b1", "shelf1"));
        // And a shelf the folder's map does not name for this address at all is
        // ground the book has left whatever it is.
        assert!(converts_on_move_to(state, "b1", "elsewhere"));
    }

    #[test]
    fn a_reorder_on_the_book_s_own_rung_copies_nothing() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // The cheapest drag in the library must stay the cheapest: re-ordering
        // the books a folder placed, on the rung it placed them on, is the
        // folder's own business. Copying here would spend a reader's disk on a
        // move that changed no ground at all.
        assert!(!converts_on_move_to(state, "b1", "shelf3"));
    }

    #[test]
    fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // "All" is the library's own list rather than a shelf, so no folder's
        // map can name it — this is the departure the rule always had.
        assert!(converts_on_move_to(state, "b1", ALL_SHELF));
        // And a virtual shelf is the reader's own by construction.
        assert!(converts_on_move_to(state, "b1", "mine"));
    }

    #[test]
    fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut folder = nested(7);
        // The shelf is gone, so the map no longer names a rung for the address.
        // The ledger is still waiting on the fingerprint, which is what makes
        // this a departure rather than a book nobody answers for: an empty map
        // answer and an empty list of folders are not the same fact.
        folder.shelf_map.remove("Fiction/SciFi");
        state.library.folders.set(vec![folder]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(converts_on_move_to(state, "b1", "shelf3"));
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut folder = nested(7);
        folder.opts.groups = false;
        // Everything lands flat, so the root rung is the ground for the whole
        // tree and a drag between the folder's own shelves is a re-order.
        folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
        state.library.folders.set(vec![folder]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
        assert!(!converts_on_move_to(state, "b1", "flat"));
        assert!(converts_on_move_to(state, "b1", "shelf2"));
    }

    #[test]
    fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut copying = nested(7);
        copying.id = "f2".into();
        copying.opts.in_place = false;
        state.library.folders.set(vec![nested(7), copying]);
        state.library.books.set(vec![
            linked_at("b1", "/books/Fiction/SciFi/dune.md", 7),
            // The copy a departure made: the library's own, so it simply moves.
            stored_at("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
            // A loose file the reader dropped: no ledger is waiting on it.
            linked_at("b3", "/elsewhere/loose.md", 11),
        ]);

        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(!converts_on_move_to(state, "b2", "shelf2"));
        assert!(!converts_on_move_to(state, "b3", "shelf2"));
        // A row the library does not hold owes nothing at all.
        assert!(!converts_on_move_to(state, "gone", "shelf2"));
    }

    /// A shelf cut from a watched folder's tree, which is what makes a landing
    /// on it look like a return.
    fn folder_shelf(id: &str, folder_id: &str, rel: &str) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: Some(rel.to_string()),
            },
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }
    }

    /// `f1` read in place, holding fingerprint 7, with the moved-out log a
    /// departure of that file writes: the name the shelf showed, the rung it was
    /// filed on, and no row bound to it yet.
    fn folder_with_moved_log() -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(7)]),
            ignored: vec![Tombstone {
                fp: fp(7),
                title: Some("Dune".to_string()),
                format: Format::Markdown,
                last_path: "/books/Fiction/SciFi/dune.md".to_string(),
                shelf_id: Some("shelf3".to_string()),
                removed_ms: 5,
                moved: true,
                returned_row: None,
            }],
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            scanned_ms: 0,
        }
    }

    #[test]
    fn a_departure_s_landing_does_not_bind_the_log_it_just_wrote() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        // The row a departure leaves behind: the library's own copy, still
        // wearing the name the folder's log remembers.
        let mut rows = vec![stored_at("b1", "/books/Fiction/SciFi/dune.md", "/store/b1.md", 9)];
        find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
        state.library.books.set(rows);
        state
            .library
            .shelves
            .set(vec![folder_shelf("shelf2", "f1", "Fiction")]);
        state.library.folders.set(vec![folder_with_moved_log()]);
        let bound = |state: AppState| {
            state.library.folders.with_untracked(|folders| {
                folders[0].ignored[0].returned_row.is_some()
            })
        };

        // The gesture that made the copy lands it on ANOTHER rung of the same
        // folder — which is the shape of a return without being one. Binding
        // here would spend the log on the row that just left, and from then on
        // every import of the OS file would light the copy up instead of
        // bringing the linked book home to the rung it belongs on.
        move_row(state, "b1", "shelf2", None, true);
        assert!(!bound(state), "a departure is not a return");
        let filed = state.library.shelves.with_untracked(|shelves| {
            shelves[0].books.iter().any(|id| id == "b1")
        });
        assert!(filed, "and the move itself still happened");

        // The NEXT drag of the same row back is the return the bind exists for,
        // and it binds exactly as it always did.
        move_row(state, "b1", "shelf2", None, false);
        assert!(bound(state), "a later gesture binds the log to the row by name");
    }
}
