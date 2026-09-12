//! The library's two automatic measurements: the startup pass over every
//! address the library holds, and the rescan of every watched folder when the
//! window regains focus. The order between them is a property of this module:
//! a diff against placeholder fingerprints would add a second copy of every
//! migrated book, so the measure runs first and calls the rescan itself.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{apply_check, book_rows};
use library_core::folder::FolderOpts;
use library_core::wire::PathCheck;

use super::claim::claim_root;
use super::folder::run_folder;
use super::gate::RootPlan;
use super::tasks::task_id;
use super::Asked;
use crate::services::library as wire;
use crate::state::AppState;

/// Re-scan every watched folder. Called on startup (from [`verify_library`],
/// never before it) and whenever the window regains focus.
pub fn rescan_watched(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // A library still carrying placeholder fingerprints cannot be diffed: a real
    // fingerprint matches no placeholder, so every book a watched folder already
    // holds would be added a second time. `verify_library` measures first and
    // calls this itself once it has.
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
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
        // A folder a previous run is still walking keeps its walk: a rescan is
        // a question, and the run in flight is already answering it.
        let Some(claim) = claim_root(&root) else {
            continue;
        };
        let task = task_id();
        spawn_local(async move {
            let _claim = claim;
            run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
        });
    }
}

/// Measure every address the library holds, once, at startup.
///
/// This is what makes `missing` true, what replaces a migrated book's
/// placeholder fingerprint with a real one, and therefore what has to happen
/// before any rescan. It ends by calling [`rescan_watched`] itself, so that
/// order is a property of this function rather than of whoever remembers to call
/// the two in the right sequence.
pub fn verify_library(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // The addresses to ask about: a link has none, and a pointer at a book is
    // as alive or as dead as the book it points at, which the book's own row
    // is already in this list to answer for.
    let paths: Vec<String> = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).map(|b| b.path().to_string()).collect());
    if paths.is_empty() {
        rescan_watched(state);
        return;
    }
    spawn_local(async move {
        match wire::verify_paths(paths).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
        rescan_watched(state);
    });
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
// The import itself.
// ---------------------------------------------------------------------------

// A book about to be placed is a `(id, found file)` pair, and the id is minted
// BEFORE any copy happens, because the stored file is named after it: minting
// afterwards would leave two imports of a folder that holds a `report.pdf`
// fighting over one `report_0.pdf` in the store.
