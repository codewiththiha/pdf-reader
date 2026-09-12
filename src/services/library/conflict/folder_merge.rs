//! The compact sheet's question: one file of a folder import merging into a
//! shelf the level already held, whose NAME a rung of that shelf carries.
//! Three answers — one book, replace, or two books — and the switch that
//! gives every waiting file of the merge the same answer in one click.

use leptos::prelude::*;

use library_core::book::{find_book_mut, find_by_id};
use library_core::scan::FoundFile;

use super::{answer_batch, member_slot, minted_name, AskKind, ConflictAsk};
use crate::services::library::arrange::{purge_books, PurgeOpts};
use crate::services::library::covers;
use crate::state::AppState;

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
            crate::services::library::import::settle_ledger(state, folder_id.as_deref(), file.fp);
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
        crate::services::library::import::land_file(state, &file, name, &shelf_id, index);
        crate::services::library::import::settle_ledger(state, folder_id, file.fp);
        covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
        return;
    }
    // A folder that copies: the landing is the import module's own
    // single-file copy composition — the copy made and measured BEFORE the
    // row is promised, the row's identity the copy's own, the ledger write
    // riding the success — and a copy that fails leaves the shelf untouched
    // and the ledger unmarked, the one honest outcome for a file that could
    // not be filed.
    let fp = file.fp;
    crate::services::library::import::land_stored_copy_settling(
        state,
        file,
        name,
        shelf_id,
        index,
        folder_id.map(|id| (id.to_string(), fp)),
    );
}

// ---------------------------------------------------------------------------
// The covered file's answers: a loose import from a read-at-place folder.
// ---------------------------------------------------------------------------
