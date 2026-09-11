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
//! *Add as new* places the arrival under the next free name, so both rows are
//! books and each is a book of its own. *Make link* places a pointer row
//! ([`library_core::book::Row::Link`]) instead of a copy: a row on this shelf
//! that opens the book wherever it lives, holds no fingerprint, no resume
//! point and no highlights, and is invisible to every content check the
//! library runs.
//!
//! A MOVE is two books the reader already has, so its answers are about which
//! of them the level keeps. *Merge* folds the moved row into the one already
//! here — the survivor keeps its id, its name and its memberships, and takes
//! the further place in it, the gaps the other row can fill, its shelves and
//! its highlights. *Replace* sends the row that was here out of the library and
//! seats the arrival in its slot and on every other shelf it was filed on. *As
//! new* is the import's naming on a row that already exists: the moved row
//! takes the next free name and lands beside the one it collided with.
//!
//! Neither set has a second ask, and the reason differs per set. An import's
//! answers cannot destroy anything — the worst one can do is add a row. A
//! move's Replace can, so its row says what goes before the click: the name of
//! the row, and how many highlights leave with it.
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
use library_core::book::{Book, Fingerprint, book_rows_mut, find_by_id, find_row, fold_books};
use library_core::conflict::{
    Answer, Arrival, MoveAnswer, collide, next_name, next_shelf_name,
};
use library_core::folder::FolderOpts;
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf;

use super::arrange::{PurgeOpts, memberships, purge_books};
use crate::state::{AppState, Toast};

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
    /// Whether this ask came out of a folder import merging into a shelf the
    /// level already held. Its sheet is the compact per-file one — one book,
    /// replace, as new, with an apply-to-all switch — rather than the import's
    /// own: the reader has already answered the shelf's question, and what is
    /// left is a row of files each with the same three doors.
    pub folder_merge: bool,
    /// Whether the merging folder reads in place or copies: a linked answer
    /// lands now, a stored one lands after its copy — and a copy that fails
    /// leaves the shelf untouched.
    pub in_place: bool,
    /// The watched folder whose ledger records the placement when the answer
    /// lands, so a later rescan stays quiet about the file and a removal that
    /// was holding it out is spent.
    pub folder_id: Option<String>,
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
                let existing_name = find_row(&rows, &existing_id)
                    .map(|row| row.display_name())
                    .unwrap_or_else(|| arrival.name.clone());
                asks.push(ConflictAsk {
                    arrival,
                    existing_id,
                    existing_name,
                    folder_merge: false,
                    in_place: true,
                    folder_id: None,
                });
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
            // The link wears the name of the book it points at, which is what
            // makes the row recognisable beside it, and falls back to the
            // arrival's own name if the row went while the sheet was up — a
            // link with no name is a row the shelf cannot label, and
            // `library_core::book::sanitize` drops one.
            let name = state.library.row_name(&ask.existing_id);
            let name = if name.trim().is_empty() {
                ask.arrival.name.clone()
            } else {
                name
            };
            state
                .library
                .add_link(&name, &ask.existing_id, &ask.arrival.shelf_id);
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
    }
    advance(state);
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
    if let Some(gone_book) = gone_book {
        state.library.books.update(|rows| {
            if let Some(keep) = book_rows_mut(rows).find(|b| b.id == survivor) {
                fold_books(keep, &gone_book);
            }
        });
    }
    let inherited: Vec<String> = memberships(state, &gone_id)
        .into_iter()
        .map(|(id, _)| id)
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
    super::arrange::move_row(
        state,
        &moved_id,
        &ask.arrival.shelf_id,
        seat.or(ask.arrival.index),
    );
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if inherited.contains(&one.id) {
                shelf::shelf_add(one, &moved_id);
            }
        }
    });
    crate::storage::persist_library(state.library);
}

/// The slot a row holds on one shelf, which is the slot its replacement takes.
fn member_slot(state: AppState, shelf_id: &str, row_id: &str) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .and_then(|s| s.books.iter().position(|m| m == row_id))
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
/// its own row, and as a book of its own when the address is one the library
/// already reads ([`library_core::book::Book::independent`]), so the second
/// copy's highlights and its place in it are its own rather than the first
/// one's.
fn as_new(state: AppState, ask: &ConflictAsk) {
    let name = {
        let (rows, shelves) = state.library.snapshot_rows();
        next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
    };
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
            super::import::land_file(
                state,
                file,
                Some(name),
                &ask.arrival.shelf_id,
                ask.arrival.index,
            );
            // The row may read from an address the cover cache has no art
            // for; `land_file` leaves the queue to its caller, and this is a
            // landing of one. The persist is the caller's for the same reason.
            super::covers::backfill_missing(state);
            crate::storage::persist_library(state.library);
        }
    }
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
}

/// The reader's answer to a folder's name collision.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShelfAnswer {
    /// Mint the arriving folder's shelf under the next free name
    /// ([`next_shelf_name`]) and import into its own tree.
    AsNew,
    /// Place nothing and import nothing: leave a pointer row at the level —
    /// the folder's own `library_core::book::Row::Link`, whose target is the
    /// shelf's id — and a tap on it reveals the shelf it names, lit, wherever
    /// it hangs.
    Link,
    /// The arriving folder IS the shelf that is here: its books join it, and
    /// the files whose names it already holds ask, one by one, on the compact
    /// sheet ([`answer_folder_merge`]).
    Merge,
}

/// Put the folder question on screen. One question, no queue: a folder import
/// is one run, and the run does not start until it is answered.
pub fn raise_shelf(state: AppState, ask: ShelfConflictAsk) {
    state.library.shelf_conflict.set(Some(ask));
    state.library.shelf_conflict_open.set(true);
}

/// One of the folder sheet's three buttons.
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
                    into: None,
                },
            );
        }
        ShelfAnswer::Link => {
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
            super::import::proceed_folder(
                state,
                ask.root,
                ask.opts,
                super::import::RootPlan {
                    rename: None,
                    into: Some(ask.existing_id),
                },
            );
        }
    }
}

/// Cancel the folder question: the import simply does not run, which is what
/// Cancel has always meant.
pub fn cancel_shelf(state: AppState) {
    state.library.shelf_conflict.set(None);
    state.library.shelf_conflict_open.set(false);
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
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    if !ask.folder_merge {
        return;
    }
    apply_folder_merge(state, &ask, answer);
    advance(state);
    if !apply_all {
        return;
    }
    while let Some(next) = state.library.conflict.get_untracked() {
        if !next.folder_merge {
            break;
        }
        apply_folder_merge(state, &next, answer);
        advance(state);
    }
}

/// One answer, applied: the row heals, the file replaces it, or the file
/// lands beside it under a name of its own.
fn apply_folder_merge(state: AppState, ask: &ConflictAsk, answer: FolderMergeAnswer) {
    let Some(file) = ask.arrival.file.clone() else {
        return;
    };
    match answer {
        FolderMergeAnswer::Merge => {
            let existing = ask.existing_id.clone();
            state.library.books.update(|rows| {
                if let Some(book) =
                    book_rows_mut(rows).find(|b| b.id == existing)
                {
                    book.fp = file.fp;
                    book.fp_pending = false;
                    book.missing = false;
                }
            });
            settle_folder_ledger(state, ask, file.fp);
            crate::storage::persist_library(state.library);
        }
        FolderMergeAnswer::AsNew => {
            let name = {
                let (rows, shelves) = state.library.snapshot_rows();
                next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
            };
            land_answer_file(state, ask, file, Some(name), None);
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
            land_answer_file(state, ask, file, None, slot);
        }
    }
}

/// Land one answered file on the merged-into shelf: now, when the folder
/// reads in place, or after the store copy the folder's options owe.
fn land_answer_file(
    state: AppState,
    ask: &ConflictAsk,
    file: FoundFile,
    name: Option<String>,
    index: Option<usize>,
) {
    let shelf_id = ask.arrival.shelf_id.clone();
    if ask.in_place {
        super::import::land_file(state, &file, name, &shelf_id, index);
        settle_folder_ledger(state, ask, file.fp);
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
    let folder_id = ask.folder_id.clone();
    let fp = file.fp;
    spawn_local(async move {
        match super::copy_one_to_store(&task, &file.path, &book_id).await {
            Ok(store) => {
                super::import::mint_stored_row(
                    state, book_id, &file, store, name, &shelf_id, index,
                );
                if let Some(folder_id) = folder_id {
                    state.library.folders.update(|folders| {
                        if let Some(folder) = folders.iter_mut().find(|f| f.id == folder_id) {
                            ledger::restore_deleted(folder, &fp);
                            folder.mark_placed(fp);
                        }
                    });
                }
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
fn settle_folder_ledger(state: AppState, ask: &ConflictAsk, fp: Fingerprint) {
    let Some(folder_id) = ask.folder_id.clone() else {
        return;
    };
    state.library.folders.update(|folders| {
        if let Some(folder) = folders.iter_mut().find(|f| f.id == folder_id) {
            ledger::restore_deleted(folder, &fp);
            folder.mark_placed(fp);
        }
    });
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
        let (id, first) = state.library.reveal.get_untracked().expect("a reveal");
        assert_eq!(id, "b1");
        assert!(!state.library.conflict_open.get_untracked());

        // Asking again is asking again: the nonce is what makes a second
        // reveal of the same row a second gesture rather than an equal value
        // nobody is told about.
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        raise(state, vec![ask]);
        answer(state, Answer::GoToExisting);
        let (_, second) = state.library.reveal.get_untracked().expect("a second reveal");
        assert_ne!(first, second);
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
}
