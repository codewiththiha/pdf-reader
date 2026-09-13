//! The covered file's two answers: a loose import of a file an in-place
//! folder's tree already holds a living book for. The library's own copy on
//! this level, or the folder's book lit where it stands — and never a second
//! linked row of one read-at-place file.
//!
//! This is the ask kind that rides the unified placement vocabulary: the two
//! answers it offers are [`Placement::COVERED`] — *keep both* (the library's own
//! copy on this level) and *open* (the book the folder holds, lit) — and
//! [`super::apply_placement`] applies them. Not [`Placement::FILE`] even though
//! the arrival is a file: the three a loose import is offered include *make
//! link*, and a second linked row of one read-at-place file is the one thing this
//! folder's rule can never make.
//!
//! What is left in this file is the drain and nothing else — the one thing only
//! this question knows, which folder's tree holds the file, is carried on the ask
//! ([`AskKind::Covered`]) and printed by the sheet.

use library_core::conflict::Placement;

use super::{answer_batch, apply_placement, AskKind, ConflictAsk};
use crate::state::AppState;

/// One of the covered sheet's two buttons.
///
/// `apply_all` is the switch beside them, with the compact folder-merge sheet's
/// contract: checked, the answer is given to every covered question in the queue
/// as well — a reader who dropped forty files of one folder knows after the
/// first what the other thirty-nine are. A question that is NOT a covered one
/// stops the drain: it belongs to another gesture and gets its own sheet.
pub fn answer_covered(state: AppState, answer: Placement, apply_all: bool) {
    answer_batch(state, answer, apply_all, AskKind::is_covered, apply_one);
}

/// One answer, applied by the unified placement apply.
fn apply_one(state: AppState, ask: &ConflictAsk, answer: Placement) {
    apply_placement(state, &ask.placement(), answer);
}
