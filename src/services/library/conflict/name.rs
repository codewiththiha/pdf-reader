//! The level's own name question, and the four answers it has: an import's
//! three (go to the row that is here, add as new, make a link) and a move's
//! four (merge, replace, as new, and the link that keeps both books of one
//! file). The row that survives, the row that dissolves, and where the
//! highlights of both end up.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use ai_core::gloss::GlossMark;
use library_core::book::{find_book_mut, find_by_id, fold_books, Book};
use library_core::conflict::{Answer, MoveAnswer};
use library_core::shelf;

use super::{advance, member_slot, minted_name, ConflictAsk};
use crate::services::library::arrange::{
    converts_on_move_to, convert_to_stored, drop_row, memberships, move_row, purge_books,
    unlist_row, write_moved_stones, PurgeOpts,
};
use crate::services::library::covers;
use crate::services::library::toast;
use crate::state::AppState;

/// One of the three buttons.
pub fn answer(state: AppState, answer: Answer) {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    match answer {
        // Nothing to place: the reader asked to be shown the row they already
        // have, which is the library's own reveal — its shelf, then its card.
        Answer::GoToExisting => crate::services::library::reveal::reveal_book(state, &ask.existing_id),
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
    drop_row(state, &gone_id);
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
            // A copy that failed leaves the row linked and writes no log, so
            // there is nothing for the seating to stand aside from either.
            let departed = match convert_to_stored(state, &moved_id).await {
                Ok(()) => true,
                Err(message) => {
                    toast(state, message);
                    false
                }
            };
            covers::backfill_missing(state);
            seat_replace(state, &moved_id, &shelf_id, index, &inherited, departed);
        });
        return;
    }
    seat_replace(state, &moved_id, &shelf_id, index, &inherited, false);
}

/// The seating half of a replace: the arrival takes the survivor's slot and
/// every other shelf the survivor was filed on, and the blob is written once
/// for the whole of it.
///
/// `departed` is the replace's own copy of the gate's answer — the arrival
/// became the library's own copy in this gesture — and it travels because a
/// departure must not bind the moved-out log it just wrote. See
/// [`crate::services::library::arrange::move_row`].
fn seat_replace(
    state: AppState,
    moved_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    inherited: &[String],
    departed: bool,
) {
    move_row(state, moved_id, shelf_id, index, departed);
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
            // `false`: nothing has departed yet, and if the move turns out to
            // be a departure its own gate says so on the way back in.
            move_row(
                state,
                row_id,
                &ask.arrival.shelf_id,
                ask.arrival.index,
                false,
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
            crate::services::library::import::land_stored_copy(
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
    unlist_row(state, &gone_id);
    // The pointer wears the survivor's name, which is what makes the row
    // recognisable beside the book it points at — the import answer's rule, and
    // the one spelling of it.
    add_link_at_target(state, ask, &survivor);
    crate::storage::persist_library(state.library);
}
