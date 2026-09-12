//! The sheet's *replace*: the books the standing shelf holds leave the
//! library through the removal's own sweep, and the arriving folder's copies
//! take the shelf — with the root's claim asked FIRST, so a walk already in
//! flight refuses the run before anything is removed.

use std::collections::HashSet;

use leptos::prelude::*;

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::ledger;
use library_core::shelf::{self as shelves_ops};

use super::claim::{already_importing, root_is_claimed};
use super::gate::{proceed_folder, RootPlan};
use crate::services::library::arrange::{purge_books, PurgeOpts};
use crate::state::AppState;

/// The stored arrival's *replace* over a shelf that is NOT the folder's own
/// in-place tree: the books the level's shelf holds leave the library through
/// the removal's own sweep — rows, memberships, covers, highlights, and the
/// store copies the app made — and the folder's copies take the shelf, the
/// walk filing into it as the merge's plan does.
///
/// The claim is asked FIRST, the ordering `replace_folder_with_copies` gives
/// for the tree's own replace: a walk already in flight refuses the run
/// before anything is removed, because a removal no import re-lands is the
/// one outcome this ordering exists to prevent.
pub(crate) fn replace_shelf_with_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    existing_id: String,
) {
    if root_is_claimed(&root) {
        already_importing(state, &root);
        return;
    }
    let doomed: Vec<String> = {
        let (rows, shelves) = state.library.snapshot_rows();
        shelves_ops::members_of(&rows, &shelves, &existing_id)
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    if !doomed.is_empty() {
        arrange::purge_books(state, &doomed, PurgeOpts::default());
    }
    proceed_folder(
        state,
        root,
        opts,
        RootPlan {
            into: Some(existing_id),
            ..Default::default()
        },
    );
}

/// The rows a *replace* of the folder's own read-at-place tree would take
/// out: the folder's own linked books. A stored book on one of its shelves —
/// a copy that came home — is NOT among them: the replace is about the
/// instances that read the OS folder, and a copy the library already owns is
/// exactly what the shelf ends up holding.
pub fn replace_rows_of_tree(state: AppState, root: &str) -> Vec<String> {
    let placed: HashSet<Fingerprint> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .find(|f| f.root == root && f.opts.in_place)
            .map(|f| f.placed.clone())
            .unwrap_or_default()
    });
    if placed.is_empty() {
        return Vec::new();
    }
    state
        .library
        .books
        .with_untracked(|rows| ledger::linked_rows_of(rows, &placed))
}

/// The replace answer's first half: the folder's linked books leave the
/// library through the removal's own sweep — row, memberships, cover,
/// highlights, and a tombstone per book in the folder's ledger. The copy
/// import that follows spends those logs as it lands, so the shelf comes
/// back holding only the library's copies, in the names the shelves showed.
pub(crate) fn purge_folder_linked_books(state: AppState, root: &str) {
    let doomed = replace_rows_of_tree(state, root);
    if !doomed.is_empty() {
        arrange::purge_books(state, &doomed, PurgeOpts::default());
    }
}

/// The *replace* of the folder's own read-at-place tree, whole: the linked
/// books go through the sweep, and the folder walks again as the copies the
/// reader asked for.
///
/// The claim is asked FIRST, and the check and the claim run in one
/// synchronous step (the webview is single-threaded, and nothing awaits
/// between them): a walk already in flight — a focus rescan of this very
/// folder is the realistic one — refuses the run with the sentence the
/// double-import always gets, and the purge simply does not happen. A
/// removal no import re-lands is the one outcome this ordering exists to
/// prevent.
pub(crate) fn replace_folder_with_copies(state: AppState, root: String, opts: FolderOpts) {
    if root_is_claimed(&root) {
        already_importing(state, &root);
        return;
    }
    purge_folder_linked_books(state, &root);
    proceed_folder(state, root, opts, RootPlan::default());
}
