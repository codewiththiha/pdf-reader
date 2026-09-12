//! The moves a reader makes by hand: a drag between shelves, a lift out to
//! the root, a second membership. The membership half of the module's one
//! rule — a move never touches a file the reader owns — and the gate every
//! hand-move rides when the row it holds reads in place
//! ([`super::departure`]).

use leptos::prelude::*;

use library_core::book::{Row, find_row};
use library_core::conflict::Arrival;
use library_core::shelf::{self as shelf, ALL_SHELF, shelf_add};

use crate::services::library::conflict;
use crate::state::AppState;

use super::departure::{bind_returned, convert_departures};

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
/// import half of a screen is [`crate::services::library::import::land_stored_copy`]'s business
/// instead; nothing in this module raises one.
fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// Whether the row became the library's own copy in THIS gesture.
///
/// The question the landing owes an answer to, and a value rather than a
/// boolean at the call site: a departure writes a moved-out log, and the copy
/// then lands — often on another shelf of the very folder it left. A landing
/// that read `true` as "bind the log" would spend the log on the row that
/// just wrote it, and the file could never come home again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Departed {
    /// This gesture converted the row: it lands WITHOUT binding the folder's
    /// moved-out log — a departure is not a return.
    ThisGesture,
    /// Nothing converted here: a stored book landing on a shelf of the folder
    /// it left is a return, and the log binds to it.
    No,
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
    departed: Departed,
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
                    let departed = if gone.contains(&one) {
                        Departed::ThisGesture
                    } else {
                        Departed::No
                    };
                    move_row(state, &one, &landed_on, index, departed);
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
    if departed == Departed::No {
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

/// Re-order the library's own list, which IS the "All" level.
///
/// Lifted out and put back in together rather than one at a time: each book's
/// removal shifts the tail left, so moving four in sequence would have the second
/// one's index mean something the first one's already changed.
pub(super) fn reorder_root(rows: &mut Vec<Row>, row_ids: &[String], index: Option<usize>) {
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
pub(super) fn place_many(members: &mut Vec<String>, book_ids: &[String], index: Option<usize>) {
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
pub(super) fn insert_many<T>(list: &mut Vec<T>, items: impl Iterator<Item = T>, index: Option<usize>, shift: usize) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
}
