//! A dead address, re-pointed: the reader picks a file and the book reads it
//! from there — a linked book takes the address, a stored book takes a fresh
//! copy of it, made before anything is written so a failure leaves the row
//! exactly as it was.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Origin, find_book_mut};

use crate::services::library::covers::{self, prune_now};
use crate::services::library::toast;
use crate::services::library as wire;
use crate::state::AppState;

/// Re-point a book whose address died at a file the reader picks.
///
/// A linked book takes the new address. A stored book does NOT become linked —
/// that would quietly turn "the app keeps its own copy" back into "the app reads
/// your folder again" — so the pick is copied into the store once more, from
/// wherever the file lives now, and the copy is made BEFORE anything is written:
/// a failure to copy leaves the row exactly as it was.
fn relink_book(state: AppState, book_id: String, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        let checks = match wire::verify_paths(vec![path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return toast(state, message),
        };
        let Some(fp) = checks.first().and_then(|c| c.fingerprint()) else {
            return toast(
                state,
                "That file is not there any more. Pick the book's current location.".to_string(),
            );
        };
        let origin = state.library.books.with_untracked(|rows| {
            library_core::book::find_by_id(rows, &book_id).map(|b| b.origin.clone())
        });
        let Some(origin) = origin else {
            return;
        };

        let store = match origin {
            Origin::Linked { .. } => None,
            Origin::Stored { .. } => {
                let task = format!("relink-{book_id}");
                match wire::copy_one_to_store(&task, &path, &book_id).await {
                    Ok(store) => Some(store),
                    Err(message) => return toast(state, message),
                }
            }
        };

        state.library.books.update(|rows| {
            let Some(book) = find_book_mut(rows, &book_id) else {
                return;
            };
            match &mut book.origin {
                Origin::Linked { src } => *src = path.clone(),
                Origin::Stored { src, store: at } => {
                    *src = Some(path.clone());
                    if let Some(store) = store.as_ref() {
                        *at = store.clone();
                    }
                }
            }
            book.heal(fp);
        });
        // Covers are keyed by address, so the old entry now belongs to nobody;
        // the prune drops it and the next open renders the new one.
        prune_now(state);
        crate::storage::persist_library(state.library);
        crate::storage::persist_covers(state.library);
        // The old address's cover belongs to nobody now, and the new one has
        // never been rendered: queue it rather than waiting for an open.
        covers::backfill_missing(state);
    });
}

/// Ask the reader for a file and relink to it.
///
/// The picker is the engine's own (`pdf_engine::api::pick_document`) rather than
/// a second dialog implementation here: it is the same question — "which
/// document?" — with the same filter, and a cancel is the same non-event.
pub fn relink_dialog(state: AppState, book_id: String) {
    spawn_local(async move {
        match pdf_engine::api::pick_document().await {
            Ok(path) => relink_book(state, book_id, path),
            // A cancel is the reader changing their mind, not a failure.
            Err(message) if message == "Open cancelled" => {}
            Err(message) => toast(state, message),
        }
    });
}
