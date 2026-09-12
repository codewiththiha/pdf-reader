//! The covered file's two answers: a loose import of a file an in-place
//! folder's tree already holds a living book for. The library's own copy on
//! this level, or the folder's book lit where it stands — and never a second
//! linked row of one read-at-place file.

use library_core::conflict::Arrival;

use super::{answer_batch, raise, screen, AskKind, ConflictAsk};
use crate::services::library::reveal;
use crate::state::AppState;

/// The two answers to the covered-file sheet: the library's own copy on this
/// level, or the book the folder already holds.
///
/// Not a variant of [`Answer`] because it is not the name question: the
/// level's names have not been consulted yet, and the file's own ground is
/// what asks. A linked second instance is not among the answers and cannot
/// be — the folder reads that file in place, and one OS file is one linked
/// book, whichever shelf it is standing on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoveredAnswer {
    /// Import on this level anyway: the file is copied into the library's
    /// store and lands as a book of its own — an instance the library owns,
    /// not bound to the folder's OS tree, so none of the folder's read-at-
    /// place rules apply to it. The landing still walks the level's name
    /// question, because a stored copy is a row like any import.
    ImportHere,
    /// Place nothing: go to the folder's book and light it up, wherever in
    /// the folder's tree it stands — the answer that means *I did not intend
    /// to add anything*, which is what importing a file the library already
    /// reads in place usually means.
    GoToExisting,
}

/// One of the two buttons on the covered-file sheet.
///
/// `apply_all` is the switch beside them, with the compact folder-merge
/// sheet's contract: checked, the answer is given to every covered question
/// in the queue as well — a reader who dropped forty files of one folder
/// knows after the first what the other thirty-nine are. A question that is
/// NOT a covered one stops the drain: it belongs to another gesture and gets
/// its own sheet.
pub fn answer_covered(state: AppState, answer: CoveredAnswer, apply_all: bool) {
    answer_batch(state, answer, apply_all, AskKind::is_covered, apply_covered);
}

/// One answer, applied: the folder's book lit, or the library's own copy on
/// the way into the level.
fn apply_covered(state: AppState, ask: &ConflictAsk, answer: CoveredAnswer) {
    match answer {
        CoveredAnswer::GoToExisting => reveal::reveal_book(state, &ask.existing_id),
        CoveredAnswer::ImportHere => {
            let Some(file) = ask.arrival.file.clone() else {
                return;
            };
            // The copy the reader chose still meets the level's own names on
            // the way in: a clean arrival copies now, and one whose name the
            // level holds joins the queue behind this sheet as the question
            // it always was. The copy itself is the import module's — made
            // before the row is promised, measured as the row is minted.
            let (clean, conflicts) = screen(
                state,
                vec![Arrival::import(
                    file.clone(),
                    ask.arrival.shelf_id.clone(),
                    ask.arrival.index,
                )],
            );
            if !clean.is_empty() {
                crate::services::library::import::land_stored_copy(
                    state,
                    file,
                    None,
                    ask.arrival.shelf_id.clone(),
                    ask.arrival.index,
                );
            }
            raise(state, conflicts);
        }
    }
}
