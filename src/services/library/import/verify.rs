//! The library's two automatic measurements, in the one order they owe:
//! FIRST a pass over every address the library holds — what turns a book
//! `missing` when its file was deleted or moved out from under it, and what
//! replaces a migrated book's placeholder fingerprint with a real one — and
//! THEN the walk of every watched folder when the window regains focus. The
//! walk alone only ever SEES what is still on disk: a shelf that gained a
//! book while another quietly died would be a shelf lying about one of them,
//! so both automatic moments run both passes, in this order, in one task.
//! Diffing a library that is still carrying placeholder fingerprints would
//! add a second copy of every book a watched folder already holds, which is
//! why the measure runs first and the walk's guard reads what it left.
//!
//! The watch a HAND turns is here too, at the bottom, for the reason the two
//! automatic moments are one function: turning a folder's watch on is asking
//! for a walk of it, and the walk it owes is the same quiet rescan the focus
//! owes — same claim, same ledger table, same answer about a book the reader
//! removed. A second spelling of "walk one folder now" would be a second place
//! for the two to disagree about what a rescan skips.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{apply_check, book_rows};
use library_core::folder::{self as folder_ops, FolderOpts};
use library_core::shelf::{self as shelves_ops};
use library_core::wire::PathCheck;

use super::claim::claim_root;
use super::folder::run_folder;
use super::gate::RootPlan;
use super::tasks::task_id;
use super::Asked;
use crate::services::library::{folder_label, picker_focus, toast};
use crate::services::library as wire;
use crate::state::AppState;

/// Measure every address the library holds, then walk every watched folder.
/// Called on startup (as [`verify_library`]) and whenever the window regains
/// focus; both moments owe both passes, measure first.
pub fn rescan_watched(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // A link has no address to measure, and a pointer at a book is as alive
    // or as dead as the book it points at, which the book's own row is
    // already in this list to answer for.
    let paths: Vec<String> = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).map(|b| b.path().to_string()).collect());
    spawn_local(async move {
        if !paths.is_empty() {
            match wire::verify_paths(paths).await {
                Ok(checks) => apply_checks(state, &checks),
                Err(message) => {
                    web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
                }
            }
        }
        run_watched(state);
    });
}

/// The walk half: claim every watched folder's root and start its quiet run.
/// The measurement pass has just replaced every placeholder it could, so the
/// guard here only holds the walk back for a book whose address the shell
/// could not read at all — a diff against a placeholder would add a second
/// copy of every book the folder already holds.
fn run_watched(state: AppState) {
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
        return;
    }
    // A focus the app's own picker caused is not a reader coming back to the
    // window: the import that picker closed on is about to walk this very
    // ground, and better — it is the run that lifts a tombstone, which this one
    // honours. The measure pass above still ran, because a book whose file died
    // while a dialog was up is a book the library should know about; it is the
    // walk that waits for a focus that means it.
    if picker_focus() {
        return;
    }
    let watched: Vec<(String, FolderOpts)> = state
        .library
        .folders
        .get_untracked()
        .iter()
        .filter(|f| f.opts.watch)
        .map(|f| (f.root.clone(), f.opts.clone()))
        .collect();
    for (root, opts) in watched {
        walk_one(state, root, opts);
    }
}

/// Walk ONE folder now, quietly, as the rescan it is: no card unless it found
/// something, no toast for a folder that cannot be read, and the ledger's
/// rescan table, where the tombstones a removal wrote still hold.
///
/// Two callers and one spelling, because the two are the same walk asked by two
/// different moments — every watched folder when the window regains focus, and
/// the one folder a hand just turned a watch on.
fn walk_one(state: AppState, root: String, opts: FolderOpts) {
    // A folder a previous run is still walking keeps its walk: a rescan is
    // a question, and the run in flight is already answering it. An explicit
    // run in flight keeps its walk for the stronger reason — it is the reader's.
    let Some(claim) = claim_root(&root, Asked::OnFocus) else {
        return;
    };
    let task = task_id();
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
    });
}

/// The startup's name for the same two passes [`rescan_watched`] runs:
/// measure every address the library holds, then walk the watched folders.
/// One function under two names because the two moments read differently —
/// a launch owes the reader a library that knows what it holds, a focus owes
/// a shelf that noticed the folder — and the work is one.
pub fn verify_library(state: AppState) {
    rescan_watched(state);
}

/// Measure one address and write the result.
///
/// Called when a book joins the library through the reader rather than through an
/// import: an open proves the file is there and measures nothing, so the row it
/// leaves behind carries a placeholder identity — and a placeholder is exactly
/// what [`rescan_watched`] refuses to diff against. One file's metadata is a
/// cheap way to keep a hand-opened book from holding every watched folder off
/// until the next launch.
pub fn verify_one(state: AppState, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        match wire::verify_paths(vec![path]).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
    });
}

/// Write a batch of path checks into the library. Split out of
/// [`verify_library`] because a relink asks for exactly the same thing about one
/// address, and one definition of "what a measurement does to a book" is one
/// fewer place for the two to disagree.
pub(super) fn apply_checks(state: AppState, checks: &[PathCheck]) {
    let mut changed = false;
    state.library.books.update(|rows| {
        for check in checks {
            if !apply_check(rows, check).is_empty() {
                changed = true;
            }
        }
    });
    if !changed {
        return;
    }
    crate::storage::persist_library(state.library);
}

// ---------------------------------------------------------------------------
// The watch a hand turns.
// ---------------------------------------------------------------------------

/// The watch a shelf answers for: which folder owns it, whether it is on, and
/// what that folder is called wherever the library names one.
///
/// A value rather than a `bool` because the menu row that asks is a label and a
/// sentence, and a caller that fetched the folder a second time to spell them
/// would be a second reader of a ledger the first one just read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfWatch {
    /// The folder the flag belongs to, which is the whole tree rather than the
    /// rung that was asked: `watch` is one answer about one ground.
    pub folder_id: String,
    pub on: bool,
    pub label: String,
}

/// The watch a SHELF answers for, when it answers for one: the shelf stands on
/// ground a folder the library reads in place owns, so the folder's watch is a
/// fact about the ground under this shelf rather than about the shelf itself.
///
/// Two shelves answer, and the second is the reason this is a walk and not a
/// lookup. A shelf the folder's own tree cut — its root, or any rung of it,
/// however deep — answers with that folder. A shelf the reader MADE inside such
/// a tree answers with the closest folder shelf above it: it is not a rung, and
/// the directory it holds is not one the folder walks, but it is standing inside
/// a watched tree, and "stop watching" asked from there is the same ask as from
/// the rung at the top. The row names the folder it is about, so the two cannot
/// be mistaken for a watch of one shelf.
///
/// `None` for a shelf with no read-at-place folder above it — the reader's own
/// shelf on the reader's own ground, which has no watch to turn — and for a
/// shelf of a COPYING folder: the import sheet does not offer the watch beside a
/// copy, so the shelf's menu does not either, and the two surfaces that can set
/// the flag stay one rule.
pub fn shelf_watch(state: AppState, shelf_id: &str) -> Option<ShelfWatch> {
    let folder_id = state.library.shelves.with_untracked(|shelves| {
        let own = shelves_ops::find(shelves, shelf_id).and_then(|s| s.kind.folder_id());
        own.or_else(|| {
            // Root first, so the LAST of them is the closest: the tree a hand
            // made its shelf inside, rather than one it happens to hang under.
            shelves_ops::ancestors(shelves, shelf_id)
                .iter()
                .rev()
                .find_map(|shelf| shelf.kind.folder_id())
        })
        .map(str::to_string)
    })?;
    let folder = state.library.folder(&folder_id)?;
    if !folder.opts.in_place {
        return None;
    }
    Some(ShelfWatch {
        on: folder.opts.watch,
        label: folder_label(&folder.root),
        folder_id,
    })
}

/// Turn a folder's watch on or off, from the shelf's own menu.
///
/// The flag is the whole of the write, and it is the FOLDER's rather than the
/// shelf's: every card, breadcrumb and menu row that draws the watch dot reads
/// the folder, so one write answers for the whole tree without being told which
/// rungs it has.
///
/// Turning it ON owes a walk, and owes it quietly: the reader just asked the
/// library to look at this folder, so a file that arrived while nobody was
/// watching should show up now rather than at the next focus. The walk is the
/// rescan's own, which is the point of it being [`walk_one`] rather than an
/// import — the tombstones a removal wrote still hold, because turning a watch
/// on is not asking back for the books the reader took out, and handing them
/// over would be the resurrection a tombstone exists to prevent. Turning it OFF
/// writes nothing else: the ledger stays exactly as it was, so the books this
/// folder placed are still the books it placed if the watch ever comes back.
pub fn set_folder_watch(state: AppState, folder_id: &str, on: bool) {
    let Some((root, opts, label)) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, folder_id).and_then(|folder| {
            // A folder that already answers this way is not a write, and is not
            // a walk either: toggling it on again would be a second rescan of a
            // ground the first one has just covered.
            (folder.opts.watch != on).then(|| {
                let mut opts = folder.opts.clone();
                opts.watch = on;
                (folder.root.clone(), opts, folder_label(&folder.root))
            })
        })
    }) else {
        return;
    };
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            folder.opts.watch = on;
        }
    });
    crate::storage::persist_library(state.library);
    toast(
        state,
        if on {
            format!("Watching {label} for new books.")
        } else {
            format!("{label} is no longer watched for new books.")
        },
    );
    if on {
        walk_one(state, root, opts);
    }
}
