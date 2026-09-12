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
//! A SHELF read at its place departs the same way, and asks first: a rung a
//! hand takes off the seat its folder's tree names becomes the library's own
//! copy — the shelf turns into the reader's own, the read-at-place books
//! standing on the departing rungs go through the very [`convert_to_stored`] a
//! book's departure rides, the copy takes the next free name at the level it
//! lands on so the folder's own name stays free for the tree the next import
//! re-mints, and the folder's map lets the departed zone go. The copies cost
//! the reader's disk, and a cost is a question: the move goes to a sheet
//! before it happens ([`ShelfDepartureAsk`]), and a cancel does not land it.
//! A shelf of a copying folder, and every shelf the reader owns, still moves
//! as the membership edit it always was.
//!
//! The sheet has a third answer when the drop landed inside the mover's
//! FAMILY — a rung of an in-place tree that covers the ground the mover stands
//! on — because a move inside the tree a read-at-place shelf belongs to never
//! has to cost a copy: the shelf goes back to the place its folder names
//! instead, the displaced folder by the import's own fold, the off-seat rung
//! by the reseat that gives the disk its place back. A read-at-place shelf
//! lives on the seat its directory stands on: off the seat it is a copy, and
//! on it, wherever the hand found it, it is the folder itself.
//!
//! The rule is also what makes the ledger's hardest row true without anybody
//! having to remember it: dragging a book off a watched folder's shelf leaves
//! its fingerprint in that folder's `placed` set — and a departure that
//! converted leaves a moved-out log beside it — so the next rescan skips the
//! file instead of filing it straight back where the reader just moved it
//! from, and a later import of it knows what it is bringing home.
//!
//! ## The file map
//!
//! | module | the question |
//! | --- | --- |
//! | [`moves`] | a hand moving books: the drag, the lift out, the second membership |
//! | [`departure`] | a read-at-place book leaving its ground as the library's own copy |
//! | [`purge`] | a removal, and the seven things that have to go with it |
//! | [`shelves`] | the reader's own shelves: made, named, nested, reordered, taken apart |
//! | [`shelf_departure`] | a read-at-place SHELF leaving its seat: the ask, the copies, the ways home |
//! | [`relink`] | a dead address, re-pointed at a file the reader picks |

mod departure;
mod moves;
mod purge;
mod relink;
mod shelf_departure;
mod shelves;

#[cfg(test)]
mod tests;

pub use moves::{also_show, file_many, move_many_to_shelf, move_row, unfile_books};
pub use purge::{purge_books, PurgeOpts};
pub use relink::{ask_relink, cancel_relink, relink_dialog, relink_search_folder};
pub use shelf_departure::{
    answer_departure_return, cancel_departure, confirm_departure, ReturnPath, SeamSide,
    ShelfDepartureAsk,
};
pub use shelves::{
    create_shelf_and_enter, create_shelf_here, delete_shelf, memberships, nest_many, nest_shelf,
    rename_shelf, reorder_shelves_to_anchor,
};

pub(crate) use departure::{convert_to_stored, converts_on_move_to, write_moved_stones};
pub(crate) use moves::Departed;
pub(crate) use purge::{drop_row, unlist_row};

use library_core::shelf::{self as shelf, Shelf};

/// The first of one folder's shelves a book is filed on, in shelf order.
///
/// One answer rather than every answer, because a removed book comes back to ONE
/// shelf and a tombstone that listed three would have to choose at restore time
/// with less information than it has now.
pub(super) fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|s| s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}
