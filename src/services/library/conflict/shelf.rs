//! The folder's own question, asked BEFORE the walk: the level already holds
//! the NAME the arriving folder would wear. Which answers the sheet offers is
//! the arrival's mode — a stored arrival gets the level's three, a
//! read-at-place arrival gets the pointer and the merge.

use leptos::prelude::*;

use library_core::conflict::next_shelf_name;
use library_core::folder::FolderOpts;
use library_core::shelf;

use crate::services::library::reveal;
use crate::services::library::toast;
use crate::state::AppState;

/// The folder question on screen: the name arriving, and the shelf already
/// here wearing it.
///
/// A separate ask rather than a variant of [`ConflictAsk`] because a folder
/// has no [`Arrival`] — nothing has been measured when its NAME is the
/// question — and because its answers are about a whole import run rather
/// than about one placement: the ones that import start the run again with a
/// plan, and the ones that do not walk away with a light or a pointer.
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
    /// the sheet words it as one. Own or not, the arrival's MODE decides the
    /// answers: a stored arrival gets the level's own three — go and look at
    /// the shelf that is here, replace it, or a shelf of the next free name
    /// — and a read-at-place arrival of a DIFFERENT folder's name keeps the
    /// pointer and the merge, its *as new* withheld as the second instance
    /// the family gate exists to prevent.
    pub own: bool,
}

/// The reader's answer to a folder's name collision.
///
/// Which of them the sheet offers is the arrival's mode: a STORED arrival —
/// copies the library owns, unrelated to any tree — gets *show it*,
/// *replace* and *as new*, the level's own three; a READ-AT-PLACE arrival of
/// a different folder's name gets *make link* and *merge*, its *as new*
/// withheld as the second instance the family gate exists to prevent. A
/// read-at-place arrival of its OWN family never reaches the sheet at all:
/// the gate answers it with a light, a continuation, or the fold back into
/// the tree its directory names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShelfAnswer {
    /// Import nothing and go and look: the library navigates to the shelf
    /// that holds the name and lights it up where it stands — the folder
    /// import's own spelling of the book sheet's *already imported*, and the
    /// stored arrival's answer for "oh, that one".
    Show,
    /// Mint the arriving folder's shelf under the next free name
    /// ([`next_shelf_name`]) and import into its own tree. Of ground the
    /// library already reads in place, the tree it mints holds copies of its
    /// own — independent books of their own bytes beside the linked ones the
    /// old tree keeps reading.
    AsNew,
    /// Place nothing and import nothing: leave a pointer row at the level —
    /// the folder's own `library_core::book::Row::Link`, whose target is the
    /// shelf's id — and a tap on it reveals the shelf it names, lit, wherever
    /// it hangs. The read-at-place arrival's answer.
    Link,
    /// The arriving folder IS the shelf that is here: its books join it, and
    /// the files whose names it already holds ask, one by one, on the compact
    /// sheet ([`answer_folder_merge`]). The read-at-place arrival's other
    /// answer.
    Merge,
    /// The destructive answer, and only a stored arrival's sheet offers it:
    /// the books the shelf that is here holds leave the library through the
    /// removal's own sweep, and the folder's copies take the shelf, so what
    /// stands at the end is one shelf of the library's own copies. Of the
    /// folder's OWN read-at-place tree it is the log-spending sweep the
    /// import module owns; of any other shelf it is the removal's receipt
    /// over the shelf's members and the merge's filing into it. The row says
    /// what goes before the click — how many books leave, highlights and all
    /// — the way the move sheet's replace does.
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
            crate::services::library::import::proceed_folder(
                state,
                ask.root,
                ask.opts,
                crate::services::library::import::RootPlan {
                    rename: Some(name),
                    ..Default::default()
                },
            );
        }
        ShelfAnswer::Show => {
            // Import nothing and go and look: the light lands on the shelf
            // that holds the name, wherever it hangs — the stored arrival's
            // "oh, that one", answered the way the family gate answers a
            // pick of ground the library already reads.
            reveal::reveal_shelf(state, &ask.existing_id);
        }
        ShelfAnswer::Link => {
            // A pointer at the shelf, on the level the import would have
            // minted one: the row the reader can recognise, and no second
            // door with the same name on it.
            state
                .library
                .add_link(&ask.existing_name, &ask.existing_id, shelf::ALL_SHELF);
            toast(state, format!("Linked to {}.", ask.existing_name));
        }
        ShelfAnswer::Merge => {
            // The arriving folder's books join the shelf that is here, and
            // the files whose names it already holds ask one by one on the
            // compact sheet.
            crate::services::library::import::proceed_folder(
                state,
                ask.root,
                ask.opts,
                crate::services::library::import::RootPlan {
                    into: Some(ask.existing_id),
                    ..Default::default()
                },
            );
        }
        ShelfAnswer::Replace => {
            // The folder's OWN read-at-place tree is the import module's own
            // sweep: the root's claim first, so a walk already in flight
            // refuses the answer BEFORE anything is removed, then the linked
            // books go and the copy walk spends the logs it wrote. Any other
            // shelf — a stored folder's, a reader's own — is the removal's
            // receipt over the shelf's members and the copies filing into it.
            let own_in_place = ask.own
                && state.library.folders.with_untracked(|folders| {
                    folders
                        .iter()
                        .any(|f| f.root == ask.root && f.opts.in_place)
                });
            if own_in_place {
                crate::services::library::import::replace_folder_with_copies(state, ask.root, ask.opts);
            } else {
                crate::services::library::import::replace_shelf_with_folder(
                    state,
                    ask.root,
                    ask.opts,
                    ask.existing_id,
                );
            }
        }
    }
}

/// Cancel the folder question: the import simply does not run, which is what
/// Cancel has always meant.
pub fn cancel_shelf(state: AppState) {
    state.library.shelf_conflict.set(None);
    state.library.shelf_conflict_open.set(false);
}
