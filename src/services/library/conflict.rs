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

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use ai_core::gloss::GlossMark;
use library_core::book::{
    Book, Fingerprint, Row, find_book_mut, find_by_id, find_row, fold_books,
};
use library_core::conflict::{
    Answer, Arrival, MoveAnswer, collide, next_name, next_shelf_name,
};
use library_core::folder::{self as folder_ops, FolderOpts};
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf;

use super::arrange::{
    PurgeOpts, converts_on_move_to, convert_to_stored, memberships, purge_books, toast,
    write_moved_stones,
};
use crate::state::library::{AlreadyNote, NoteKind};
use crate::state::{AppState, Toast};

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

/// One of the three buttons.
pub fn answer(state: AppState, answer: Answer) {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    match answer {
        // Nothing to place: the reader asked to be shown the row they already
        // have, which is the library's own reveal — its shelf, then its card.
        Answer::GoToExisting => super::reveal::reveal_book(state, &ask.existing_id),
        Answer::AsNew => as_new(state, &ask),
        Answer::AsLink => {
            let target = ask.existing_id.clone();
            add_link_at_target(state, &ask, &target);
        }
    }
    advance(state);
}

/// One of the three buttons on a MOVE's sheet.
///
/// The arrival is a row the reader is holding, so every answer here writes that
/// row rather than minting one — and an ask whose arrival names no row (it went
/// while the sheet was up) has nothing to write, so it is answered by moving on.
pub fn answer_move(state: AppState, answer: MoveAnswer) {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    if ask.arrival.moving.is_none() {
        advance(state);
        return;
    }
    match answer {
        MoveAnswer::Merge => merge(state, &ask),
        MoveAnswer::Replace => replace(state, &ask),
        MoveAnswer::AsNew => as_new(state, &ask),
        MoveAnswer::Link => link_move(state, &ask),
    }
    advance(state);
}

/// Whether the row that survives is the library's own copy OF the row that
/// dissolves: a stored book whose recorded provenance is the other's address.
///
/// One question in one place, because the two answers that dissolve a row — a
/// merge into the copy and a link at it — both write the folder's moved-out log
/// on this condition and on nothing else. A log written for a same-name merge of
/// two DIFFERENT books would keep a file out of every folder that placed it, and
/// a folder that never held the file would have no restore row to offer back.
fn survivor_is_the_copy_of(state: AppState, survivor: &str, gone: &Book) -> bool {
    state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, survivor).is_some_and(|keep| {
            keep.origin.is_store_copy_of(gone.path())
        }))
}

/// Merge: the row already on the level survives and the moved row dissolves
/// into it.
///
/// The survivor is the row the reader can already see here, and its id is what
/// every shelf holding it and every key in storage already names, so it is the
/// one that stays. The order of the three writes is the whole of the care this
/// takes: the marks move while both rows can still be read, because the sweep a
/// removal rides takes the dissolving row's list with it and a fold that ran
/// afterwards would be a merge that deleted one side's highlights; then the
/// rows fold, by [`fold_books`]; then the memberships the dissolving row held
/// become the survivor's, and the row itself goes.
///
/// One membership is not inherited, and it is the level the arrival names as
/// its departure: a drag from "t" onto "s" is a move OFF "t", so "t" is not
/// one of the shelves the survivor takes over. Inheriting it would put the
/// survivor on the shelf the reader just lifted the book from, which reads as
/// a move that did not happen — the book is still there under the name it
/// always had — and only a second drag of the survivor, which collides with
/// nothing because it is already on the level it is dropped on, would take it
/// off. A filing has no departure to honour, so it inherits every shelf the
/// dissolved row held.
fn merge(state: AppState, ask: &ConflictAsk) {
    let survivor = ask.existing_id.clone();
    let Some(gone_id) = ask.arrival.moving.clone() else {
        return;
    };
    fold_marks(state, &survivor, &gone_id);
    let gone_book = state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, &gone_id).cloned());
    if let Some(gone_book) = &gone_book {
        state.library.books.update(|rows| {
            if let Some(keep) = find_book_mut(rows, &survivor) {
                fold_books(keep, gone_book);
            }
        });
    }
    // A read-at-place book folding into the library's own stored copy of ITS
    // content leaves the folder's file with no row to answer for it: the
    // folder takes a moved-out log bound to the survivor, so a later import
    // of the file highlights the copy the reader just called the one book,
    // instead of minting a linked neighbour beside it. The provenance `src`
    // is the check that it IS that content — a same-name merge of two
    // different books writes no log, because there the file's own book is
    // exactly what an import should bring back.
    if let Some(gone) = &gone_book
        && survivor_is_the_copy_of(state, &survivor, gone)
    {
        write_moved_stones(state, gone, Some(&survivor));
    }
    let inherited: Vec<String> = memberships(state, &gone_id)
        .into_iter()
        .map(|(id, _)| id)
        // The level the move left is the one shelf the survivor does NOT take
        // over: the departure is the point of the move, and a merge that filed
        // the survivor back on the source would leave the book visibly where
        // the reader moved it from. A filing names no departure and inherits
        // every shelf, which is what a second membership means.
        .filter(|id| ask.arrival.from.as_deref() != Some(id.as_str()))
        .collect();
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if inherited.contains(&one.id) {
                shelf::shelf_add(one, &survivor);
            }
        }
    });
    super::arrange::drop_row(state, &gone_id);
}

/// Replace: the row that was on the level goes, and the arrival takes its
/// place.
///
/// Its SLOT and not the tail, because a replace is an overwrite and an
/// overwrite stays where the thing it replaced was — and every OTHER shelf the
/// displaced row was filed on, because a replace that quietly took a book off
/// shelves the question never mentioned is a removal the reader did not ask
/// for. What it does take is the row's own: its name, its resume point, its
/// highlights and the store copy when the app made one, which is what the
/// sheet's row says before the click.
fn replace(state: AppState, ask: &ConflictAsk) {
    let Some(moved_id) = ask.arrival.moving.clone() else {
        return;
    };
    // Read the world before writing any of it: the slot and the memberships
    // are both facts about the row that is about to go.
    let seat = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
    let inherited: Vec<String> = memberships(state, &ask.existing_id)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    purge_books(
        state,
        std::slice::from_ref(&ask.existing_id),
        PurgeOpts::default(),
    );
    let shelf_id = ask.arrival.shelf_id.clone();
    let index = seat.or(ask.arrival.index);
    // A read-at-place arrival becomes the library's own copy before it is
    // seated, and the seating waits for the copy — the survivor is already
    // gone, so the arrival seats even if the copy fails: it is the only book
    // left standing, linked or not.
    if tauri_bridge::has_tauri() && converts_on_move_to(state, &moved_id, &shelf_id) {
        spawn_local(async move {
            if let Err(message) = convert_to_stored(state, &moved_id).await {
                toast(state, message);
            }
            super::covers::backfill_missing(state);
            seat_replace(state, &moved_id, &shelf_id, index, &inherited);
        });
        return;
    }
    seat_replace(state, &moved_id, &shelf_id, index, &inherited);
}

/// The seating half of a replace: the arrival takes the survivor's slot and
/// every other shelf the survivor was filed on, and the blob is written once
/// for the whole of it.
fn seat_replace(
    state: AppState,
    moved_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    inherited: &[String],
) {
    super::arrange::move_row(state, moved_id, shelf_id, index);
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if inherited.contains(&one.id) {
                shelf::shelf_add(one, moved_id);
            }
        }
    });
    crate::storage::persist_library(state.library);
}

/// Put a pointer at `target` on the ask's level, wearing the target's own name.
///
/// One spelling for the two answers that leave a link behind — an import's *make
/// link* and a move's *link* — because the name is the whole of what makes the row
/// recognisable beside the book it points at, and a fallback each answer spelled
/// itself is a fallback the two could disagree about. A target that went while the
/// sheet was up has no name left to give, and a link with no name is a row the
/// shelf cannot label — one `library_core::book::sanitize` drops on the next load
/// — so the arrival's own name is the honest stand-in.
fn add_link_at_target(state: AppState, ask: &ConflictAsk, target: &str) {
    let name = state.library.row_name(target);
    let name = if name.trim().is_empty() {
        ask.arrival.name.clone()
    } else {
        name
    };
    state
        .library
        .add_link(&name, target, &ask.arrival.shelf_id);
}

/// The next free name for this ask's arrival, counted against the level it is
/// going to.
///
/// Read at the click rather than at the raise, and in one place: a shelf that
/// landed between the two is a name the promise on the row has to skip, and the
/// two answers that mint one (*as new* for an import, *as new* for a merge) have
/// to mint the same name for the same arrival.
fn minted_name(state: AppState, ask: &ConflictAsk) -> String {
    let (rows, shelves) = state.library.snapshot_rows();
    next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

/// The slot a row holds on one shelf, which is the slot its replacement takes.
fn member_slot(state: AppState, shelf_id: &str, row_id: &str) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, shelf_id).and_then(|s| s.books.iter().position(|m| m == row_id))
    })
}

/// Both rows' highlights under the survivor's key, and nothing else: the marks
/// keep their ids, and the AI answers ride the ids, so a mark that travels
/// arrives with the answer it already had.
///
/// Two rows of ONE address already share one list, and there is nothing to
/// move — which is the common case, and the reason this reads the keys rather
/// than assuming they differ.
fn fold_marks(state: AppState, survivor_id: &str, gone_id: &str) {
    let (into, from) = state.library.books.with_untracked(|rows| {
        (
            find_by_id(rows, survivor_id).map(Book::gloss_key),
            find_by_id(rows, gone_id).map(Book::gloss_key),
        )
    });
    let (Some(into), Some(from)) = (into, from) else {
        return;
    };
    if into == from {
        return;
    }
    let all = crate::storage::load_gloss();
    let mine = all.get(&into).cloned().unwrap_or_default();
    let Some(theirs) = all.get(&from).filter(|marks| !marks.is_empty()) else {
        return;
    };
    crate::storage::persist_gloss(&into, &union_marks(&mine, theirs));
}

/// The union of two mark lists by spot identity: everything `base` holds, in
/// its order, plus every mark of `extra` denoting a spot `base` has not
/// marked. [`GlossMark::same_spot`] is the identity — the same rule a capture
/// dedupes by and a re-click toggles by — so a merged shelf agrees with the
/// page it renders on about what "the same mark" is.
fn union_marks(base: &[GlossMark], extra: &[GlossMark]) -> Vec<GlossMark> {
    let mut union: Vec<GlossMark> = base.to_vec();
    for mark in extra {
        if !union.iter().any(|kept| kept.same_spot(mark)) {
            union.push(mark.clone());
        }
    }
    union
}

/// Add as new: the arrival takes the next free name on that level and lands.
///
/// A moved row is renamed and then moved — the rename is what frees the
/// collision, and a move that did not rename would ask the same question
/// again on the way in. An imported file is landed under the minted name as
/// the library's own stored copy, and as a book of its own when the address
/// is one the library already reads ([`library_core::book::Book::independent`]),
/// so the second copy's highlights and its place in it are its own rather
/// than the first one's.
fn as_new(state: AppState, ask: &ConflictAsk) {
    let name = minted_name(state, ask);
    match &ask.arrival.moving {
        Some(row_id) => {
            state.library.rename_row(row_id, &name);
            super::arrange::move_row(
                state,
                row_id,
                &ask.arrival.shelf_id,
                ask.arrival.index,
            );
        }
        None => {
            let Some(file) = ask.arrival.file.as_ref() else {
                return;
            };
            // A file's as-new is the loose import's own landing under the
            // minted name: a stored copy of the library's own, made before
            // the row is promised. The copy carries the cover queue and the
            // persist with it, the way every stored landing does.
            super::import::land_stored_copy(
                state,
                file.clone(),
                Some(name),
                ask.arrival.shelf_id.clone(),
                ask.arrival.index,
            );
        }
    }
}

/// Make link, a move's: the row the reader dragged dissolves into a pointer
/// at the row that is here.
///
/// The third answer for a read-at-place book meeting the library's own stored
/// copy of a name: the copy stays, the file on disk stays, and the level gains
/// a row that reaches it instead of a second book. Two things ride the
/// dissolution. The dragged row's highlights STAY under its address — the file
/// is still the folder's, and an import that brings the linked book back
/// should bring its marks with it, which a sweep here would have deleted. And
/// the folder takes a moved-out log bound to the survivor when the survivor is
/// a copy of that very file — the provenance `src` is the check — so a later
/// import of the file highlights the copy instead of minting a neighbour. A
/// different book of the same name gets no log: `placed` already keeps the
/// rescan quiet, and a re-import should bring the dragged book itself back.
fn link_move(state: AppState, ask: &ConflictAsk) {
    let Some(gone_id) = ask.arrival.moving.clone() else {
        return;
    };
    let survivor = ask.existing_id.clone();
    let gone_book = state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, &gone_id).cloned());
    if let Some(book) = &gone_book
        && survivor_is_the_copy_of(state, &survivor, book)
    {
        write_moved_stones(state, book, Some(&survivor));
    }
    // The pointers at the dissolved row go with it, as they do in every
    // removal: a link at nothing is a row that renders, is clicked and does
    // nothing. `unlist_row` is the one spelling of that.
    super::arrange::unlist_row(state, &gone_id);
    // The pointer wears the survivor's name, which is what makes the row
    // recognisable beside the book it points at — the import answer's rule, and
    // the one spelling of it.
    add_link_at_target(state, ask, &survivor);
    crate::storage::persist_library(state.library);
}

/// The question on screen is answered: the next one up, or the sheet closes.
fn advance(state: AppState) {
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

/// The folder question on screen: the name arriving, and the shelf already
/// here wearing it.
///
/// A separate ask rather than a variant of [`ConflictAsk`] because a folder
/// has no [`Arrival`] — nothing has been measured when its NAME is the
/// question — and because its answers are about a whole import run rather
/// than about one placement: two of the three start the run again with a
/// plan, and the third walks away with a pointer.
#[derive(Clone, PartialEq)]
pub struct ShelfConflictAsk {
    /// What the arriving folder would be called: the last segment of its
    /// path, the name the reader picked it by.
    pub incoming_name: String,
    /// The shelf already at the level, which *make link* points at and
    /// *merge* files into.
    pub existing_id: String,
    pub existing_name: String,
    /// The import the question interrupted, kept whole: an answer runs it.
    pub root: String,
    pub opts: FolderOpts,
    /// Whether the shelf that holds the name is the arriving folder's OWN —
    /// the one its previous run minted, which its `shelf_map` still names. A
    /// re-import of one folder is a continuation rather than an arrival, and
    /// the sheet words it as one; the three answers are the three answers
    /// either way, and an *as new* tree of one folder holds that folder's
    /// books as memberships of the rows the library already holds — a second
    /// arrangement, never a second copy.
    pub own: bool,
    /// Whether this ask is the MODE SWITCH: the folder arriving as copies is
    /// a folder the library already reads in place, re-picked with the
    /// read-in-place switch off. The shelf that is here is the tree's own
    /// root, and the question is not what to name the arrival but how the
    /// library should hold it from now on — so the sheet's answers are the
    /// switch's three rather than the continuation's: *as new* (a second
    /// shelf whose books are copies of their own, the old tree untouched),
    /// *merge* (the shelf that is here keeps standing, and every book on it
    /// becomes the library's copy, data and all), and *replace* (the linked
    /// books leave the library and copies take the shelf). *Make link* is
    /// not among them: a pointer at the folder's own shelf, from an import
    /// of that very folder, points at the thing being imported.
    pub mode_switch: bool,
}

/// The reader's answer to a folder's name collision.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShelfAnswer {
    /// Mint the arriving folder's shelf under the next free name
    /// ([`next_shelf_name`]) and import into its own tree. On the mode
    /// switch's sheet, the tree it mints holds copies of its own — books of
    /// their own bytes beside the linked ones the old tree keeps reading.
    AsNew,
    /// Place nothing and import nothing: leave a pointer row at the level —
    /// the folder's own `library_core::book::Row::Link`, whose target is the
    /// shelf's id — and a tap on it reveals the shelf it names, lit, wherever
    /// it hangs. Not offered on the mode switch's sheet.
    Link,
    /// The arriving folder IS the shelf that is here: its books join it, and
    /// the files whose names it already holds ask, one by one, on the compact
    /// sheet ([`answer_folder_merge`]). On the mode switch's sheet it is the
    /// quiet shape instead: the tree continues with no per-file ask, and the
    /// run flips every book it read in place into the library's own copy,
    /// keeping each row — and with it the name, the shelves, the resume
    /// point and the highlights — exactly where it stands.
    Merge,
    /// The mode switch's destructive answer, and only its sheet offers it:
    /// the folder's read-at-place books leave the library through the
    /// removal's own sweep, and the copy import lands on the shelf they
    /// left, so what stands at the end is one shelf of the library's own
    /// copies. The row says what goes before the click — how many books
    /// leave, highlights and all — the way the move sheet's replace does.
    Replace,
}

/// Put the folder question on screen. One question, no queue: a folder import
/// is one run, and the run does not start until it is answered.
pub fn raise_shelf(state: AppState, ask: ShelfConflictAsk) {
    state.library.shelf_conflict.set(Some(ask));
    state.library.shelf_conflict_open.set(true);
}

/// One of the folder sheet's buttons.
pub fn answer_shelf(state: AppState, answer: ShelfAnswer) {
    let Some(ask) = state.library.shelf_conflict.get_untracked() else {
        return;
    };
    cancel_shelf(state);
    match answer {
        ShelfAnswer::AsNew => {
            // Counted at the click rather than at the raise: a shelf that
            // landed between the two is a name the promise has to skip.
            let name = state.library.shelves.with_untracked(|shelves| {
                next_shelf_name(shelves, None, &ask.incoming_name)
            });
            super::import::proceed_folder(
                state,
                ask.root,
                ask.opts,
                super::import::RootPlan {
                    rename: Some(name),
                    ..Default::default()
                },
            );
        }
        ShelfAnswer::Link => {
            // The mode switch does not offer a link — its sheet never renders
            // the row — and an answer it was not asked is an import nobody
            // chose: nothing runs, which is what Cancel has always meant.
            if ask.mode_switch {
                return;
            }
            // A pointer at the shelf, on the level the import would have
            // minted one: the row the reader can recognise, and no second
            // door with the same name on it.
            state
                .library
                .add_link(&ask.existing_name, &ask.existing_id, shelf::ALL_SHELF);
            state.ui.toast.set(Some(Toast::new(format!(
                "Linked to {}.",
                ask.existing_name
            ))));
        }
        ShelfAnswer::Merge => {
            if ask.mode_switch {
                // The switch's merge is the quiet continuation plus the flip:
                // the tree keeps its shelf map, the walk lands what is new,
                // and the run turns every book it read in place into the
                // library's own copy where it stands. No per-file ask — the
                // reader just answered for the whole folder.
                super::import::proceed_folder(state, ask.root, ask.opts, Default::default());
                return;
            }
            super::import::proceed_folder(
                state,
                ask.root,
                ask.opts,
                super::import::RootPlan {
                    into: Some(ask.existing_id),
                    ..Default::default()
                },
            );
        }
        ShelfAnswer::Replace => {
            // Only the mode switch's sheet offers it. The import module owns
            // the order — the root's claim first, so a walk already in
            // flight refuses the answer BEFORE anything is removed, then the
            // sweep, then the copy walk that spends the logs it wrote.
            if !ask.mode_switch {
                return;
            }
            super::import::replace_folder_with_copies(state, ask.root, ask.opts);
        }
    }
}

/// Cancel the folder question: the import simply does not run, which is what
/// Cancel has always meant.
pub fn cancel_shelf(state: AppState) {
    state.library.shelf_conflict.set(None);
    state.library.shelf_conflict_open.set(false);
}

/// Tell the reader the folder they picked is already a shelf here, and light
/// that shelf up when they acknowledge it.
///
/// Not a question: a folder the library reads in place cannot be imported
/// twice — the second import would either duplicate every book in it or
/// silently do nothing, and both read as broken. So the honest answer is a
/// sentence and a highlight, and the reveal rides the modal's close rather
/// than its open, because a light that burns its 1.6 seconds behind a modal
/// nobody has dismissed is a light nobody sees.
///
/// One raiser for both of the note's sentences, because the two are one act
/// with one difference and [`NoteKind`] is that difference:
/// [`NoteKind::Gated`] is the gate, said before any walk ran, and
/// [`NoteKind::NothingNew`] is the report of a re-import walk that reconciled
/// the tree and found every book already standing, no log coming back and
/// nothing copied. Two raisers that differed by a boolean were two places to
/// keep in step about which sentence the reader was owed.
pub fn raise_note(state: AppState, shelf_id: String, name: String, kind: NoteKind) {
    state
        .library
        .already_imported
        .set(Some(AlreadyNote { shelf_id, name, kind }));
    state.library.already_imported_open.set(true);
}

/// Acknowledge the "already imported" note. The highlight is the modal's own
/// close effect's job, so every way out — the button, the backdrop, Escape,
/// the lane — ends on the shelf being lit.
pub fn close_already_imported(state: AppState) {
    state.library.already_imported_open.set(false);
}

// ---------------------------------------------------------------------------
// The compact sheet's answers: one file of a merging folder, at a time.
// ---------------------------------------------------------------------------

/// The three answers the compact sheet offers for one arriving file whose
/// name a merged-into shelf already holds. The move sheet's three, re-spelled
/// for an arrival that has no row of its own yet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FolderMergeAnswer {
    /// One book: the row on the shelf stays and takes the arriving file's
    /// measurement — the heal a rescan would give a file it found, from a
    /// reader who just said the two are the same book.
    Merge,
    /// The row on the shelf goes — through the removal's own sweep, receipt
    /// and all — and the arriving file takes its slot.
    Replace,
    /// Two books: the file lands under the next free name, as a book of its
    /// own when the address is one the library already reads.
    AsNew,
}

/// One of the compact sheet's three buttons.
///
/// `apply_all` is the switch beside them: checked, the answer is given to
/// every folder-merge question in the queue as well — a reader who has seen
/// one file of a forty-file folder and knows what the whole folder is does
/// not owe the sheet thirty-nine more clicks. A book question that is NOT a
/// folder-merge one stops the drain: it belongs to another gesture and gets
/// its own sheet.
pub fn answer_folder_merge(state: AppState, answer: FolderMergeAnswer, apply_all: bool) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_folder_merge,
        apply_folder_merge,
    );
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
fn answer_batch<A: Copy + 'static>(
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

/// One answer, applied: the row heals, the file replaces it, or the file
/// lands beside it under a name of its own.
fn apply_folder_merge(state: AppState, ask: &ConflictAsk, answer: FolderMergeAnswer) {
    let Some(file) = ask.arrival.file.clone() else {
        return;
    };
    // The two facts a folder merge's landing needs, off the kind that is the
    // only one carrying them: whether the folder reads in place (so the answer
    // lands now) or copies (so it lands after a copy that can fail), and which
    // ledger records the placement. `answer_batch` routes only a folder merge
    // here, so any other kind is a caller bug and there is nothing honest to
    // place — answering it with a default would be a guess about a folder.
    let (in_place, folder_id) = match &ask.kind {
        AskKind::FolderMerge { in_place, folder_id } => (*in_place, folder_id.clone()),
        _ => return,
    };
    // The sheet withholds *as new* from a file whose very address a
    // read-at-place row already reads — a second row of one linked file is a
    // duplicate, and the library does not make those — but apply-to-all can
    // still carry the answer across to such a question, so the rule stands on
    // the write side too. The twin's honest equivalent of "keep both" is
    // "keep the one", which is the merge.
    let answer = match answer {
        FolderMergeAnswer::AsNew
            if in_place
                && state.library.books.with_untracked(|rows| {
                    find_by_id(rows, &ask.existing_id)
                        .is_some_and(|b| b.path() == file.path)
                }) =>
        {
            FolderMergeAnswer::Merge
        }
        other => other,
    };
    match answer {
        FolderMergeAnswer::Merge => {
            // One book — and the measurement only travels with the answer
            // when the arriving file IS the row's file, which a re-import of
            // one folder always is. A different folder's namesake is
            // another content wearing one name: the shelf's book keeps its
            // own identity, the arriving file simply does not land, and the
            // ledger mark below is what keeps the next rescan quiet about it.
            let existing = ask.existing_id.clone();
            let same_file = state.library.books.with_untracked(|rows| {
                find_by_id(rows, &existing)
                    .is_some_and(|b| b.path() == file.path)
            });
            if same_file {
                state.library.books.update(|rows| {
                    if let Some(book) = find_book_mut(rows, &existing) {
                        book.heal(file.fp);
                    }
                });
            }
            settle_folder_ledger(state, folder_id.as_deref(), file.fp);
            crate::storage::persist_library(state.library);
        }
        FolderMergeAnswer::AsNew => {
            let name = minted_name(state, ask);
            land_answer_file(
                state,
                ask.arrival.shelf_id.clone(),
                file,
                Some(name),
                None,
                in_place,
                folder_id.as_deref(),
            );
        }
        FolderMergeAnswer::Replace => {
            // Read the slot before the purge takes the row that holds it: an
            // overwrite stays where the thing it replaced was.
            let slot = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
            purge_books(
                state,
                std::slice::from_ref(&ask.existing_id),
                PurgeOpts::default(),
            );
            land_answer_file(
                state,
                ask.arrival.shelf_id.clone(),
                file,
                None,
                slot,
                in_place,
                folder_id.as_deref(),
            );
        }
    }
}

/// Land one answered file on the merged-into shelf: now, when the folder
/// reads in place, or after the store copy the folder's options owe.
fn land_answer_file(
    state: AppState,
    shelf_id: String,
    file: FoundFile,
    name: Option<String>,
    index: Option<usize>,
    in_place: bool,
    folder_id: Option<&str>,
) {
    if in_place {
        super::import::land_file(state, &file, name, &shelf_id, index);
        settle_folder_ledger(state, folder_id, file.fp);
        super::covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
        return;
    }
    // A folder that copies: the copy is made BEFORE the row is promised, and
    // a copy that fails leaves the shelf untouched and the ledger unmarked —
    // the one honest outcome for a file that could not be filed. The id is
    // minted now because the stored file is named after it.
    let book_id = library_core::id::next_id(crate::time::now_ms());
    let task = format!("merge-{book_id}");
    // Owned for the future: the ledger write is the same two writes the
    // in-place branch makes, and spelling them a second time here was a second
    // place a placement could be recorded without the removal being spent.
    let ledger = folder_id.map(str::to_string);
    let fp = file.fp;
    spawn_local(async move {
        match super::copy_one_to_store(&task, &file.path, &book_id).await {
            Ok(store) => {
                super::import::mint_stored_row(
                    state, book_id, &file, store, name, &shelf_id, index,
                );
                settle_folder_ledger(state, ledger.as_deref(), fp);
                super::covers::backfill_missing(state);
                crate::storage::persist_library(state.library);
            }
            Err(message) => state.ui.toast.set(Some(Toast::new(message))),
        }
    });
}

/// The folder's ledger half of a landed answer: the placement is recorded,
/// and a removal that was holding the file out is spent — the two writes
/// `run_folder` makes when a file lands, made here because this file landed
/// after an answer rather than after a walk.
fn settle_folder_ledger(state: AppState, folder_id: Option<&str>, fp: Fingerprint) {
    let Some(folder_id) = folder_id else {
        return;
    };
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            ledger::restore_deleted(folder, &fp);
            folder.mark_placed(fp);
        }
    });
}

// ---------------------------------------------------------------------------
// The covered file's answers: a loose import from a read-at-place folder.
// ---------------------------------------------------------------------------

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
        CoveredAnswer::GoToExisting => super::reveal::reveal_book(state, &ask.existing_id),
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
                super::import::land_stored_copy(
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

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Book, Fingerprint, Origin, Row};
    use library_core::shelf::Shelf;
    use reader_core::format::Format;

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    /// A Markdown row: the cover queue skips anything that is not a PDF, so a
    /// host test that lands a book never starts the wasm render chain.
    fn row(id: &str, title: &str, path: &str, n: u32) -> Row {
        let mut book = Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked { src: path.to_string() },
            0,
        );
        book.title = Some(title.to_string());
        Row::Book(book)
    }

    /// A book row that has been read to `page`, so a fold has something to take.
    fn row_at(id: &str, title: &str, path: &str, n: u32, page: u32) -> Row {
        let mut row = row(id, title, path, n);
        row.as_book_mut().unwrap().page = page;
        row.as_book_mut().unwrap().num_pages = 400;
        row
    }

    fn shelf(id: &str, members: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: Default::default(),
            books: members.iter().map(|m| m.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    /// A stored row: the library's own copy of a file, with the provenance a
    /// conversion writes — `src` is the address the copy was made from, and it
    /// is what tells a merge or a link that the copy and a linked book are two
    /// halves of one content.
    fn stored_row(id: &str, title: &str, src_path: &str, store: &str, n: u32) -> Row {
        let mut book = Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Stored {
                src: Some(src_path.to_string()),
                store: store.to_string(),
            },
            0,
        );
        book.title = Some(title.to_string());
        Row::Book(book)
    }

    /// An in-place folder that placed the given fingerprints — the ledger half
    /// of a read-at-place import.
    fn folder_in_place(
        id: &str,
        root: &str,
        placed: &[u32],
    ) -> library_core::folder::WatchedFolder {
        library_core::folder::WatchedFolder {
            id: id.to_string(),
            root: root.to_string(),
            opts: library_core::folder::FolderOpts::default(),
            placed: placed.iter().copied().map(fp).collect(),
            ignored: Vec::new(),
            shelf_map: Default::default(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        }
    }

    fn file(name: &str, n: u32) -> library_core::scan::FoundFile {
        library_core::scan::FoundFile {
            rel: format!("{name}.md"),
            path: format!("/incoming/{name}.md"),
            ext: "md".to_string(),
            size: u64::from(n),
            fp: fp(n),
        }
    }

    /// The state a question is asked of, with the owner that keeps its signals
    /// alive — the pair a test has to hold, because a signal written under a
    /// dropped owner is a signal nobody can read.
    fn library(rows: Vec<Row>, shelves: Vec<Shelf>) -> (Owner, AppState) {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(rows);
        state.library.shelves.set(shelves);
        (owner, state)
    }

    /// The one question a single arrival raises, when it raises one.
    fn the_ask(state: AppState, arrival: Arrival) -> ConflictAsk {
        let (clean, asks) = screen(state, vec![arrival]);
        assert!(clean.is_empty(), "the level has no room for this name");
        asks.into_iter().next().expect("a question")
    }

    #[test]
    fn a_level_with_room_for_the_name_lands_without_asking() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"]), shelf("t", &[])],
        );
        let (clean, asks) = screen(state, vec![Arrival::import(file("Report", 2), "t", None)]);
        assert_eq!(clean.len(), 1);
        assert!(asks.is_empty());
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn a_name_the_level_already_holds_asks_and_names_the_row() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        assert_eq!(ask.existing_id, "b1");
        assert_eq!(ask.existing_name, "Dune");
        // The arrival survives on the ask, because an answer is what places it.
        assert_eq!(ask.arrival.name, "dune");
        assert!(ask.arrival.file.is_some());
    }

    #[test]
    fn a_batch_lands_its_clean_half_and_queues_its_questions() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                row("b2", "Foundation", "/one/foundation.md", 2),
            ],
            vec![shelf("s", &["b1", "b2"])],
        );
        let arrivals = vec![
            Arrival::import(file("neuromancer", 3), "s", None),
            Arrival::import(file("dune", 4), "s", None),
            Arrival::import(file("foundation", 5), "s", None),
        ];
        let (clean, asks) = screen(state, arrivals);
        assert_eq!(clean.len(), 1, "the name nobody holds lands now");
        assert_eq!(clean[0].name, "neuromancer");
        assert_eq!(asks.len(), 2, "and the two that collide ask");
        assert_eq!(asks[0].existing_id, "b1");
        assert_eq!(asks[1].existing_id, "b2");

        raise(state, asks);
        assert!(state.library.conflict_open.get_untracked());
        let on_screen = state.library.conflict.get_untracked().expect("a question");
        assert_eq!(on_screen.existing_id, "b1", "one question at a time");
        assert_eq!(
            state.library.conflict_waiting.get_untracked().len(),
            1,
            "and the rest wait rather than being dropped"
        );

        // A second batch in flight joins the queue behind the one on screen
        // rather than replacing it.
        let (_clean, more) = screen(
            state,
            vec![Arrival::import(file("dune", 6), "s", None)],
        );
        raise(state, more);
        assert_eq!(
            state.library.conflict.get_untracked().map(|a| a.existing_id).as_deref(),
            Some("b1"),
            "the question on screen is still the one that was asked first"
        );
        assert_eq!(state.library.conflict_waiting.get_untracked().len(), 2);

        // Cancel drops what is waiting, which is what Cancel has always meant.
        cancel(state);
        assert!(state.library.conflict.get_untracked().is_none());
        assert!(state.library.conflict_waiting.get_untracked().is_empty());
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn already_imported_places_nothing_and_lights_the_row_it_names() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        raise(state, vec![ask]);

        answer(state, Answer::GoToExisting);

        assert_eq!(
            state.library.books.get_untracked().len(),
            1,
            "no row was written"
        );
        assert_eq!(state.library.shelf.get_untracked(), "s");
        let first = state.library.reveal.get_untracked().expect("a reveal");
        assert_eq!(first.id, "b1");
        assert!(!state.library.conflict_open.get_untracked());

        // Asking again is asking again: the nonce is what makes a second
        // reveal of the same row a second gesture rather than an equal value
        // nobody is told about.
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        raise(state, vec![ask]);
        answer(state, Answer::GoToExisting);
        let second = state.library.reveal.get_untracked().expect("a second reveal");
        assert_ne!(first.nonce, second.nonce);
    }

    #[test]
    fn make_link_puts_a_pointer_on_the_level_and_no_second_book() {
        // The collision IS the row the link will point at, so a link always
        // lands on a level that already holds a book of that name: the reader
        // wants the book reachable from here and does not want a second one.
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 1), "s", None));
        raise(state, vec![ask]);

        answer(state, Answer::AsLink);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 2, "a link is a row, and not a second book");
        let link = rows.iter().find(|r| r.is_link()).expect("a link");
        assert_eq!(link.display_name(), "Dune", "it wears the name of its book");
        assert_eq!(link.target(), Some("b1"));
        assert_eq!(link.fp(), None, "and has no content identity at all");
        let shelves = state.library.shelves.get_untracked();
        let on_s = shelves.iter().find(|s| s.id == "s").map(|s| s.books.clone());
        assert_eq!(
            on_s,
            Some(vec!["b1".to_string(), link.id().to_string()]),
            "filed on the level it was dropped on, beside the book it points at"
        );
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn a_link_never_blocks_the_next_arrival_and_never_gets_opened() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            ],
            vec![shelf("s", &["l1"])],
        );
        // The only row on the shelf is a pointer: the name is taken by nothing
        // a reader would call a book, so the arrival lands.
        let (clean, asks) = screen(state, vec![Arrival::import(file("dune", 2), "s", None)]);
        assert_eq!(clean.len(), 1);
        assert!(asks.is_empty());
        // And a pointer is not the library's row for a content: the book it
        // points at is, whichever shelf that is filed on.
        assert_eq!(state.library.row("l1").map(|r| r.is_link()), Some(true));
        assert_eq!(state.library.row_name("l1"), "Dune");
        assert_eq!(state.library.row_name("gone"), "");
    }

    #[test]
    fn merge_keeps_the_row_that_was_here_and_folds_the_other_into_it() {
        // Two books of one name on one level, and the reader says they are one
        // book: the row already here survives — its id is what every shelf and
        // every storage key names — and takes the further place in it.
        let (_owner, state) = library(
            vec![
                row_at("b1", "Dune", "/one/dune.md", 1, 12),
                row_at("b2", "Dune", "/two/dune.md", 2, 240),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"]), shelf("u", &[])],
        );
        let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Merge);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1, "two books became one");
        assert_eq!(rows[0].id(), "b1", "and the one that was here is the one that stayed");
        assert_eq!(
            rows[0].book().map(|b| b.page),
            Some(240),
            "a merge never sends a reader backwards"
        );
        let shelves = state.library.shelves.get_untracked();
        let on = |id: &str| {
            shelves
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        };
        assert_eq!(on("s"), vec!["b1".to_string()], "still on the level it was asked about");
        assert_eq!(
            on("t"),
            vec!["b1".to_string()],
            "and on every shelf the dissolved row held"
        );
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn a_merge_after_a_move_leaves_the_level_the_book_departed() {
        // The regression this guards: a drag from "t" onto "s" answered with
        // merge used to file the survivor onto "t" — the very shelf the move
        // departed — so the book looked like it had never moved, and a SECOND
        // drag (which asks nothing, because the survivor is already on "s"
        // and a row never collides with itself) was what finally took it off.
        let (_owner, state) = library(
            vec![
                row_at("b1", "Dune", "/one/dune.md", 1, 12),
                row_at("b2", "Dune", "/two/dune.md", 2, 240),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"]), shelf("u", &["b2"])],
        );
        let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None).leaving("t"));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Merge);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1, "two books became one");
        let shelves = state.library.shelves.get_untracked();
        let on = |id: &str| {
            shelves
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            on("s"),
            vec!["b1".to_string()],
            "the survivor keeps the level it was asked about"
        );
        assert_eq!(
            on("t"),
            Vec::<String>::new(),
            "and the level the move departed stays departed — that departure IS the move"
        );
        assert_eq!(
            on("u"),
            vec!["b1".to_string()],
            "every OTHER shelf the dissolved row held still joins the survivor"
        );
    }

    #[test]
    fn the_covered_question_places_nothing_and_lights_the_folders_book() {
        // A loose import of a file an in-place folder holds: the folder's
        // book is the only book this file is, so the "show" answer writes no
        // row and lights the one the folder has.
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/books/dune.md", 1)],
            vec![shelf("fs", &["b1"]), shelf("s", &[])],
        );
        let ask = ConflictAsk::covered(
            Arrival::import(file("dune", 1), "s", None),
            "b1".to_string(),
            "Dune".to_string(),
            "f1".to_string(),
        );
        raise(state, vec![ask]);

        answer_covered(state, CoveredAnswer::GoToExisting, false);

        assert_eq!(
            state.library.books.get_untracked().len(),
            1,
            "no row was written"
        );
        let id = state.library.reveal.get_untracked().expect("a reveal").id;
        assert_eq!(id, "b1", "and the light lands on the book the folder holds");
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn the_mode_switch_never_links_at_the_folder_it_is_importing() {
        // A pointer at the folder's own shelf, from an import of that very
        // folder, points at the thing being imported: the sheet does not
        // render the row, and an answer it was never asked refuses by doing
        // nothing — beyond the close every answer owes.
        let (_owner, state) = library(Vec::new(), Vec::new());
        state.library.shelf_conflict.set(Some(ShelfConflictAsk {
            incoming_name: "Books".to_string(),
            existing_id: "s1".to_string(),
            existing_name: "Books".to_string(),
            root: "/books".to_string(),
            opts: FolderOpts::default(),
            own: true,
            mode_switch: true,
        }));
        state.library.shelf_conflict_open.set(true);

        answer_shelf(state, ShelfAnswer::Link);

        assert!(
            state.library.books.get_untracked().is_empty(),
            "no pointer was written"
        );
        assert!(
            !state.library.shelf_conflict_open.get_untracked(),
            "and the sheet closed, which is all a refused answer may do"
        );
    }

    #[test]
    fn replace_sends_the_row_that_was_here_out_and_seats_the_arrival_in_its_place() {
        let (_owner, state) = library(
            vec![
                row_at("b1", "Dune", "/one/dune.md", 1, 12),
                row_at("b2", "Dune", "/two/dune.md", 2, 240),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"]), shelf("u", &["b1"])],
        );
        let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Replace);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id(), "b2", "the arrival is the book that is left");
        let shelves = state.library.shelves.get_untracked();
        let on = |id: &str| {
            shelves
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        };
        assert_eq!(on("s"), vec!["b2".to_string()], "in the displaced row's slot");
        assert_eq!(
            on("u"),
            vec!["b2".to_string()],
            "and on the other shelves it was filed on — a replace is not a quiet removal"
        );
        assert_eq!(on("t"), Vec::<String>::new(), "having left the one it was lifted from");
    }

    #[test]
    fn a_move_collision_is_the_other_question() {
        let (_owner, state) = library(
            vec![
                row_at("b1", "Dune", "/one/dune.md", 1, 12),
                row_at("b2", "Dune", "/two/dune.md", 2, 1),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        // Two books of one name and the reader is holding one of them, so the
        // sheet offers merge, replace and as new — and never a link, which is
        // an answer for an arrival that has no row of its own to keep. The two
        // answer sets are two types, so a sheet cannot offer one for the other.
        let (clean, asks) = screen(state, vec![Arrival::moved("b2", "Dune", "s", None)]);
        assert!(clean.is_empty());
        assert_eq!(asks.len(), 1);
        assert!(!asks[0].arrival.is_import());
        assert_eq!(asks[0].existing_id, "b1");
        // The same name arriving as a FILE on the same level is the other
        // question, and says so on the arrival the sheet reads.
        let (clean, asks) = screen(state, vec![Arrival::import(file("dune", 3), "s", None)]);
        assert!(clean.is_empty());
        assert!(asks[0].arrival.is_import());
    }

    #[test]
    fn add_as_new_renames_a_moved_row_and_then_moves_it() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                row("b2", "Dune", "/two/dune.md", 2),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::AsNew);

        let rows = state.library.books.get_untracked();
        let renamed = rows
            .iter()
            .find(|r| r.id() == "b2")
            .map(Row::display_name)
            .unwrap_or_default();
        assert_eq!(renamed, "Dune_1", "the counter is the answer, not a dialog");
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves.iter().find(|s| s.id == "s").map(|s| s.books.clone()),
            Some(vec!["b1".to_string(), "b2".to_string()]),
            "and it landed on the shelf it was dropped on"
        );
        assert_eq!(
            shelves.iter().find(|s| s.id == "t").map(|s| s.books.clone()),
            Some(Vec::new()),
            "having left the one it was lifted from"
        );
        // The name it took is free on the level it left, and the level it
        // joined now holds both names.
        assert_eq!(
            state.library.row_name("b1"),
            "Dune",
            "the row that was already there keeps its own name"
        );
    }

    #[test]
    fn a_link_answer_dissolves_the_dragged_row_into_a_pointer_and_binds_the_log() {
        // The pointer shape: a read-at-place book meets the library's own
        // stored copy of its content. The copy stays, the file on disk stays,
        // the level gains a row that reaches it — and the folder's moved-out
        // log binds to the survivor, so a later import of the file highlights
        // the copy instead of minting a neighbour.
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/books/dune.md", 1),
                stored_row("b2", "Dune", "/books/dune.md", "/store/b2.md", 2),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        state
            .library
            .folders
            .set(vec![folder_in_place("f1", "/books", &[1])]);
        let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Link);

        let rows = state.library.books.get_untracked();
        assert!(find_row(&rows, "b1").is_none(), "the dragged row is gone");
        let link = rows
            .iter()
            .find(|r| r.is_link())
            .expect("the level holds a pointer");
        assert_eq!(link.target(), Some("b2"), "pointing at the copy");
        let shelves = state.library.shelves.get_untracked();
        let on_t = shelves
            .iter()
            .find(|s| s.id == "t")
            .map(|s| s.books.clone());
        assert_eq!(
            on_t,
            Some(vec!["b2".to_string(), link.id().to_string()]),
            "the copy keeps its place and the pointer joins the level"
        );
        let folders = state.library.folders.get_untracked();
        let stone = folders[0]
            .ignored
            .iter()
            .find(|entry| entry.moved)
            .expect("a moved-out log");
        assert_eq!(stone.fp, fp(1), "for the file on disk");
        assert_eq!(
            stone.returned_row.as_deref(),
            Some("b2"),
            "bound to the row that represents it"
        );
    }

    #[test]
    fn a_merge_into_the_stored_copy_binds_the_folder_log_to_the_survivor() {
        // One book, the reader said — and the folder's file has no row of its
        // own any more, so the log has to name the row that answers for it.
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/books/dune.md", 1),
                stored_row("b2", "Dune", "/books/dune.md", "/store/b2.md", 2),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        state
            .library
            .folders
            .set(vec![folder_in_place("f1", "/books", &[1])]);
        let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Merge);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1, "two books became one");
        let folders = state.library.folders.get_untracked();
        let stone = folders[0]
            .ignored
            .iter()
            .find(|entry| entry.moved)
            .expect("a moved-out log");
        assert_eq!(stone.returned_row.as_deref(), Some("b2"));
    }

    #[test]
    fn a_merge_of_two_different_books_writes_no_log() {
        // The same NAME is not the same content: the folder's file still has a
        // book the reader means by it — the one a re-import brings back — and
        // a log binding the folder to an unrelated survivor would send that
        // import to the wrong row.
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/books/dune.md", 1),
                row("b2", "Dune", "/other/dune.md", 2),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        state
            .library
            .folders
            .set(vec![folder_in_place("f1", "/books", &[1])]);
        let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
        raise(state, vec![ask]);

        answer_move(state, MoveAnswer::Merge);

        let folders = state.library.folders.get_untracked();
        assert!(
            folders[0].ignored.is_empty(),
            "a different book's merge is no departure of THIS file"
        );
    }
}
