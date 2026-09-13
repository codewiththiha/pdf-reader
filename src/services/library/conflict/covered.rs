//! The two-answer question, and the two shapes that ask it: a loose import of a
//! file an in-place folder's tree already holds a living book for, and a loose
//! import of a file whose CONTENT the library already holds somewhere. Both are
//! the library's own copy on this level, or the book that is already there lit
//! where it stands — and neither is a second linked row of one read-at-place
//! file.
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

/// One of the two-answer sheet's buttons.
///
/// `apply_all` is the switch beside them, with the compact folder-merge sheet's
/// contract: checked, the answer is given to every question of this shape in the
/// queue as well — a reader who dropped forty files of one folder knows after the
/// first what the other thirty-nine are. A question that is NOT a two-answer one
/// stops the drain: it belongs to another gesture and gets its own sheet.
///
/// Both shapes drain together, and that is right rather than convenient: a drop
/// of forty files can contain both kinds, and "import my own copy" / "go to the
/// book I have" mean the same thing whichever fact the library noticed first.
pub fn answer_covered(state: AppState, answer: Placement, apply_all: bool) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_two_answer,
        apply_one,
    );
}

/// One answer, applied by the unified placement apply.
fn apply_one(state: AppState, ask: &ConflictAsk, answer: Placement) {
    apply_placement(state, &ask.placement(state), answer);
}
