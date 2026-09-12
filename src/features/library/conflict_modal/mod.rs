//! The collision sheet: the level already holds a book of this name, and the
//! question is which of three things the reader meant.
//!
//! One sheet, one question, three rows — and WHICH three is the arrival's own
//! fact, because an import and a move are different questions. A file arriving
//! has nothing of its own yet, so its answers are about what to put here:
//! *already imported* places nothing and takes the reader to the row that is
//! already there, *add as new* keeps both under the next free name, and *make
//! link* puts a pointer here instead of a copy. A row being moved is two books
//! the reader already has, so its answers are about which of them the level
//! keeps: *merge* folds the moved one into the one that is here, *replace*
//! sends the one that is here out of the library and seats the arrival in its
//! place, and *as new* keeps both under the next free name.
//!
//! Neither set gets a second ask. An import's answers cannot destroy anything,
//! so there is nothing to warn about; a move's Replace can, so its row says
//! what goes before the click — the name of the row and how many highlights
//! leave with it — which is the promise-on-the-row idiom the rest of the sheet
//! already keeps.
//!
//! One more shape wears this sheet's chrome without wearing its question: a
//! loose import of a file that sits inside a folder the library reads in
//! place asks about the FILE'S GROUND rather than the level's name — the
//! library's own stored copy here, or the book the folder holds, lit — because
//! a second link of one read-at-place file is the one thing the folder rule
//! never makes.
//!
//! The service half — what a collision is, what each answer writes — is
//! `crate::services::library::conflict` and the rule itself is
//! `library_core::conflict`; this directory is the ask.
//!
//! ## Four files, one sheet
//!
//! [`info`] is every string the sheet prints, read once per answer. [`name_sheet`]
//! is the level's own name question, [`folder_merge`] the compact per-file sheet a
//! merged folder asks, and [`covered`] the two answers a loose import of a file an
//! in-place tree already holds gets. This file is the shell and the dispatch
//! between the three — one signal, one modal, and the kind of the ask decides
//! which body renders.
//!
//! Cancel — the button, the backdrop and the Escape key — drops the question on
//! screen and every one waiting behind it, which is what a file manager's copy
//! dialog has always meant by Cancel: the placements already answered keep
//! their answers and the ones not asked simply do not land.

mod covered;
mod folder_merge;
mod info;
mod name_sheet;

use leptos::prelude::*;

use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::state::AppState;

use covered::CoveredSheet;
use folder_merge::FolderMergeSheet;
use info::NameSheetInfo;
use name_sheet::NameSheet;

/// The sheet, mounted once by the library page.
///
/// The open flag lives on the library state rather than in a provided handle
/// (the remove sheet's shape) because the raisers are services: an import asks
/// from inside a spawned future no component owns, and a signal on the state is
/// the one door every raiser and this view already share.
#[component]
pub(crate) fn ConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.conflict.open;

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question and the ones waiting
    // behind it go with it, so the sheet can never reopen onto a question
    // somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.conflict.ask.set(None);
            state.library.conflict_waiting.set(Vec::new());
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="The library already holds a book of that name here"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.conflict.ask.get()?;
                // A folder merge's file asks wear the compact sheet: the
                // shelf's question is already answered, and what is left is a
                // run of files with the same three doors each.
                if ask.kind.is_folder_merge() {
                    return Some(
                        view! { <FolderMergeSheet state=state ask=ask /> }.into_any(),
                    );
                }
                // A covered file's ask is about the file's own ground rather
                // than the level's name, and its sheet is the two answers the
                // read-at-place rule leaves.
                if ask.kind.is_covered() {
                    return Some(
                        view! { <CoveredSheet state=state ask=ask /> }.into_any(),
                    );
                }
                let info = NameSheetInfo::of(state, &ask);
                Some(view! { <NameSheet state=state info=info /> }.into_any())
            }}
        </ModalShell>
    }
}
