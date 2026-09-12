//! The "already imported?" sheet: one question, three answers.
//!
//! The RULE is not here — it is `library_core::conflict`, which is pure and
//! host-tested, and answers the only question this surface asks: does the level
//! this arrival is going to already hold a book of this name? This file is the
//! wiring between that answer and the three things a reader can do about it,
//! and it is thin on purpose. It renders nothing (that is
//! `crate::features::library::conflict_modal`), decides nothing (that is the
//! crate) and stores nothing (that is `crate::state::library`).
//!
//! ## Two arrivals, and so two questions
//!
//! Which three answers the sheet offers is decided by what is arriving, and the
//! two cases are different questions rather than one question with six answers.
//!
//! An IMPORT has nothing of its own yet — no row, no resume point, no
//! highlights — so its answers are about what to put on the level, and *already
//! imported* places nothing and reveals the row that is already there
//! — the answer that means "I did not intend to add anything", and the one
//! that used to be a book silently vanishing into the shelf it was dropped on.
//! *Add as new* places the arrival under the next free name as the library's
//! own stored copy, so both rows are books and each is a book of its own.
//! *Make link* places a pointer row
//! ([`library_core::book::Row::Link`]) instead of a copy: a row on this shelf
//! that opens the book wherever it lives, holds no fingerprint, no resume
//! point and no highlights, and is invisible to every content check the
//! library runs.
//!
//! A MOVE is two books the reader already has, so its answers are about which
//! of them the level keeps. *Merge* folds the moved row into the one already
//! here — the survivor keeps its id, its name and its memberships, and takes
//! the further place in it, the gaps the other row can fill, its shelves and
//! its highlights, less the one shelf the move DEPARTED: a merge that filed
//! the survivor back on the level the book was lifted from would leave the
//! move visibly undone. *Replace* sends the row that was here out of the library and
//! seats the arrival in its slot and on every other shelf it was filed on. *As
//! new* is the import's naming on a row that already exists: the moved row
//! takes the next free name and lands beside the one it collided with.
//!
//! Neither set has a second ask, and the reason differs per set. An import's
//! answers cannot destroy anything — the worst one can do is add a row. A
//! move's Replace can, so its row says what goes before the click: the name of
//! the row, and how many highlights leave with it.
//!
//! ## And a third question, about the file's own ground
//!
//! A loose import of a file that sits inside a folder the library READS IN
//! PLACE is neither of the questions above: the level's names have not been
//! consulted, and the arrival has no row — but the library already holds the
//! book this file is, as the folder's own linked book, and a second linked
//! row of one read-at-place file is the one thing the folder rule never
//! makes. So the file asks its own two-answer question ([`CoveredAnswer`])
//! BEFORE the name question: the library's own stored copy on this level
//! (*import here*), or the book the folder holds, lit where it stands (*show
//! the imported one*). A copy the reader chose still walks the level's names
//! on the way in. A file whose folder never placed it — new since the last
//! scan, or outside the folder's filters — is no question at all and simply
//! imports; a file the folder's log remembers REMOVING is not a question
//! either: the import spends the log and the book comes back in its folder's
//! place (see `crate::services::library::import`).
//!
//! ## Two doors
//!
//! [`screen`] splits a batch into the arrivals that may land now and the
//! questions the level has to ask, and [`raise`] puts those questions in front
//! of the reader — the first on screen and the rest waiting behind it. Every
//! placing surface walks through the pair, a drag of four books and a drop of
//! one file alike, which is why an import and a drag cannot disagree about
//! what a collision is: they build the same [`Arrival`] and hand it to the
//! same rule.
//!
//! ## One question at a time
//!
//! The sheet holds a single [`ConflictAsk`] and the rest of a batch waits on
//! [`crate::state::library::LibraryState::conflict_waiting`]: answering pops
//! the next one onto the screen, and Cancel drops them, which is what Cancel
//! has always meant — the placements already answered keep their answers and
//! the ones not asked simply do not land. There is no "apply to all" and no
//! second ask, because nothing here is destructive: the worst an answer can do
//! is add a row, and a row is removed by the sheet that says what it takes.
//!
//! ## The file map
//!
//! One file per question, mirroring the sheets that render them
//! (`crate::features::library::conflict_modal`): the queue, the screen and
//! the shared helpers live here, and each question's answers beside it.
//!
//! | module | the question |
//! | --- | --- |
//! | [`name`] | the level already holds that NAME: merge / replace / as new / link |
//! | [`folder_merge`] | one file of a merging folder, on the compact sheet |
//! | [`covered`] | a loose file of a read-at-place folder: import here / go to it |
//! | [`shelf`] | the level already holds that folder's name, before the walk |
//! | [`note`] | not a question: the sentence and the highlight |

mod covered;
mod folder_merge;
mod name;
mod note;
mod shelf;

#[cfg(test)]
mod tests;

pub use covered::{answer_covered, CoveredAnswer};
pub use folder_merge::{answer_folder_merge, FolderMergeAnswer};
pub use name::{answer, answer_move};
pub use note::{close_already_imported, raise_note};
pub use shelf::{answer_shelf, cancel_shelf, raise_shelf, ShelfAnswer, ShelfConflictAsk};

use leptos::prelude::*;

use library_core::book::{find_row, Row};
use library_core::conflict::{collide, next_name, Arrival};
use library_core::shelf;

use crate::state::AppState;

/// Which question an ask is, and the facts only that question has.
///
/// Three sheets share one queue and one signal, and which of them an ask wears
/// used to be three booleans on it — plus an `in_place` that meant nothing unless
/// the first was true and a `folder_id` that meant two different things depending
/// on the second. Two of the four answer functions then opened with a runtime
/// guard, and a caller that set the wrong combination got a sheet that silently
/// did nothing: the flags could disagree with each other and nothing in the type
/// said so. One variant per question makes the illegal combinations
/// unrepresentable, the guards unnecessary, and each question's facts visible
/// only where they mean something.
#[derive(Clone, PartialEq)]
pub enum AskKind {
    /// The level's own name question: an arrival whose name a row on that level
    /// already carries. WHICH three answers the sheet offers is the arrival's
    /// fact rather than this one's — a file gets the import's, a row being moved
    /// gets the move's — so this variant carries nothing.
    NameCollision,
    /// A per-file question out of a folder import merging into a shelf the level
    /// already held. The shelf's question is answered; what is left is a run of
    /// files with the same three doors each, and a switch that gives every
    /// waiting question the same answer in one click.
    FolderMerge {
        /// Whether the merging folder reads in place or copies: a linked answer
        /// lands now, a stored one lands after its copy — and a copy that fails
        /// leaves the shelf untouched.
        in_place: bool,
        /// The watched folder whose ledger records the placement when the answer
        /// lands, so a later rescan stays quiet about the file and a removal that
        /// was holding it out is spent.
        folder_id: Option<String>,
    },
    /// A loose import of a file that sits inside a folder the library READS IN
    /// PLACE, where the book that folder holds for it is alive and standing. Two
    /// answers rather than three — the library's own stored copy on this level,
    /// or the folder's book lit where it stands — because the third answer a name
    /// collision offers, a second row of one linked file, is the one thing a
    /// read-at-place folder can never make. It is a question about the FILE's
    /// ground rather than the level's name, so it is asked even on a level that
    /// holds nothing of that name, and asked BEFORE the name question: a copy the
    /// reader chose still meets the level's own names on the way in.
    Covered {
        /// The folder whose tree holds the file, which is the folder its sheet
        /// names. Always one: a covered ask exists because a specific tree
        /// covers the ground the file stands on.
        folder_id: String,
    },
}

impl AskKind {
    /// The watched folder whose ledger a landed answer settles, when this ask
    /// has one. Both folder kinds do, for the same reason: a placement the ledger
    /// does not know about is a book the next rescan adds again.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            AskKind::NameCollision => None,
            AskKind::FolderMerge { folder_id, .. } => folder_id.as_deref(),
            AskKind::Covered { folder_id } => Some(folder_id),
        }
    }

    /// Whether this ask's folder reads in place, so an answer lands now rather
    /// than after a copy.
    ///
    /// `false` for the two kinds that have no folder of their own: a name
    /// collision's copy is the library's own whatever the level is, and a covered
    /// ask's *import here* is always a stored copy — the file's own tree already
    /// has the linked book, which is the whole reason the question was asked.
    pub fn in_place(&self) -> bool {
        matches!(self, AskKind::FolderMerge { in_place: true, .. })
    }

    /// Whether this ask wears the compact per-file sheet.
    pub fn is_folder_merge(&self) -> bool {
        matches!(self, AskKind::FolderMerge { .. })
    }

    /// Whether this ask wears the covered file's two answers.
    pub fn is_covered(&self) -> bool {
        matches!(self, AskKind::Covered { .. })
    }
}

/// The question on screen.
#[derive(Clone, PartialEq)]
pub struct ConflictAsk {
    /// The arrival that collided, kept whole: an answer places it, and a
    /// placement needs the file it measured or the row it was moving, the
    /// level it was going to and the slot the drop pointed at.
    pub arrival: Arrival,
    /// The row already on that level whose name the arrival carries — the row
    /// *already imported* reveals and *make link* points at.
    pub existing_id: String,
    /// That row's name, read once: the sheet prints it in three places, and a
    /// heading and two buttons that need one string must not each derive their
    /// own.
    pub existing_name: String,
    /// Which question this is, and the facts only that question has.
    pub kind: AskKind,
}

impl ConflictAsk {
    /// The level's own name question, about an arrival that collided with a row
    /// already there.
    pub fn name_collision(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::NameCollision,
        }
    }

    /// One file of a folder import merging into a standing shelf, whose name a
    /// rung of that shelf already holds.
    pub fn folder_merge(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        in_place: bool,
        folder_id: String,
    ) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::FolderMerge {
                in_place,
                folder_id: Some(folder_id),
            },
        }
    }

    /// A loose import of a file an in-place tree already holds a living book for.
    pub fn covered(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        folder_id: String,
    ) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::Covered { folder_id },
        }
    }
}

/// The name a colliding row shows, or the arrival's own when the row went
/// between the collision and the read.
///
/// One spelling because four call sites ask it, and because the sheet prints the
/// answer in three places: a heading and two buttons that each derived their own
/// would eventually disagree about which book the question is about.
///
/// Crate-visible for the same reason the constructors are: a folder import builds
/// its own asks off its own snapshot of the rows, and reading the colliding row's
/// name is part of building one.
pub(crate) fn existing_name_of(rows: &[Row], existing_id: &str, arrival: &Arrival) -> String {
    find_row(rows, existing_id)
        .map(|row| row.display_name())
        .unwrap_or_else(|| arrival.name.clone())
}

/// Split a batch into the arrivals that may land now and the questions the
/// level has to ask. Every placing surface hands its placements through here
/// BEFORE writing anything — a drag, a lift out to the root, a bulk filing, a
/// loose-file import — and applies the clean half at once, so a drop of ten
/// files with two collisions files eight and asks about two.
pub fn screen(state: AppState, arrivals: Vec<Arrival>) -> (Vec<Arrival>, Vec<ConflictAsk>) {
    let (rows, shelves) = state.library.snapshot_rows();
    let mut clean = Vec::with_capacity(arrivals.len());
    let mut asks = Vec::new();
    for arrival in arrivals {
        match collide(&rows, &shelves, &arrival) {
            Some(existing_id) => {
                let existing_name = existing_name_of(&rows, &existing_id, &arrival);
                asks.push(ConflictAsk::name_collision(
                    arrival,
                    existing_id,
                    existing_name,
                ));
            }
            None => clean.push(arrival),
        }
    }
    (clean, asks)
}

/// Put questions in front of the reader: the first on screen, the rest waiting
/// behind it. A sheet already up takes them onto its queue rather than being
/// replaced — two drops in flight owe two answers, and a raise that dropped
/// the first question would be a placement vanishing exactly the way this
/// module exists to stop.
pub fn raise(state: AppState, asks: Vec<ConflictAsk>) {
    if asks.is_empty() {
        return;
    }
    let open = state.library.conflict_open.get_untracked();
    if open && state.library.conflict.get_untracked().is_some() {
        state.library.conflict_waiting.update(|waiting| {
            waiting.extend(asks);
        });
        return;
    }
    let mut asks = asks;
    let first = asks.remove(0);
    state.library.conflict_waiting.update(|waiting| {
        waiting.extend(asks);
    });
    state.library.conflict.set(Some(first));
    state.library.conflict_open.set(true);
}

/// The question on screen is answered: the next one up, or the sheet closes.
pub(super) fn advance(state: AppState) {
    let next = state
        .library
        .conflict_waiting
        .with_untracked(|waiting| waiting.first().cloned());
    match next {
        Some(ask) => {
            state.library.conflict_waiting.update(|waiting| {
                waiting.remove(0);
            });
            state.library.conflict.set(Some(ask));
        }
        None => cancel(state),
    }
}

/// Cancel — the sheet's, the backdrop's and the Escape key's one write. The
/// question on screen and every one behind it are skipped: the placements
/// already answered keep their answers, and the rest simply do not land.
pub fn cancel(state: AppState) {
    state.library.conflict.set(None);
    state.library.conflict_waiting.set(Vec::new());
    state.library.conflict_open.set(false);
}

// ---------------------------------------------------------------------------
// The shelf's own question: a folder arriving under a name the level holds.
// ---------------------------------------------------------------------------

/// The next free name for this ask's arrival, counted against the level it is
/// going to.
///
/// Read at the click rather than at the raise, and in one place: a shelf that
/// landed between the two is a name the promise on the row has to skip, and the
/// two answers that mint one (*as new* for an import, *as new* for a merge) have
/// to mint the same name for the same arrival.
pub(super) fn minted_name(state: AppState, ask: &ConflictAsk) -> String {
    let (rows, shelves) = state.library.snapshot_rows();
    next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

/// The slot a row holds on one shelf, which is the slot its replacement takes.
pub(super) fn member_slot(state: AppState, shelf_id: &str, row_id: &str) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, shelf_id).and_then(|s| s.books.iter().position(|m| m == row_id))
    })
}

/// The drain both compact sheets ride: answer the question on screen, then —
/// with *apply to all* on — every question behind it that is the SAME kind, and
/// stop at the first one that is not.
///
/// One spelling rather than one per sheet because the two sheets' contract is
/// one contract, and the half of it that is easy to get wrong is the stop: a
/// question of the other kind belongs to another gesture and owes its own sheet,
/// so the drain has to leave it at the front of the queue rather than answer it
/// with a button the reader pressed for something else. A second copy of this
/// loop is a second place to get that wrong.
pub(super) fn answer_batch<A: Copy + 'static>(
    state: AppState,
    answer: A,
    apply_all: bool,
    is_mine: fn(&AskKind) -> bool,
    apply: fn(AppState, &ConflictAsk, A),
) {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    if !is_mine(&ask.kind) {
        return;
    }
    apply(state, &ask, answer);
    advance(state);
    if !apply_all {
        return;
    }
    while let Some(next) = state.library.conflict.get_untracked() {
        if !is_mine(&next.kind) {
            break;
        }
        apply(state, &next, answer);
        advance(state);
    }
}
