//! The store batch: one `store_books` call for a run's whole copy list, the
//! per-file failure sentence the batch's toast speaks, and the one
//! measurement pass over the copies that landed.

use std::collections::HashMap;

use library_core::book::Fingerprint;
use library_core::scan::FoundFile;
use library_core::wire::{StoreRequest, StoreResult};

use crate::services::library::{file_name, toast};
use crate::services::library as wire;
use crate::state::AppState;

/// Split one batch of copy results into the addresses that landed, and say
/// something about the ones that did not.
///
/// One spelling for both batches the library copies — a folder walk's and a
/// loose file drop's — because a per-file failure is the same news either way
/// and the reader should hear it in the same words. `noun` is the only thing
/// that differs and it is what the sentence counts: the files on the way in.
///
/// A per-file failure is collected rather than fatal, which is the rule both
/// callers were already keeping: a folder with one locked file in it should
/// still import the other ninety-nine.
fn partition_store_results(
    state: AppState,
    results: Vec<StoreResult>,
    noun: &str,
) -> HashMap<String, String> {
    let mut landed = HashMap::new();
    let mut failures = Vec::new();
    for result in results {
        if result.is_ok() {
            landed.insert(result.id, result.store);
        } else {
            failures.push(file_name(&result.src));
        }
    }
    if !failures.is_empty() {
        let message = match failures.len() {
            1 => format!("Could not copy {}", failures[0]),
            n => format!("Could not copy {n} {noun}, starting with {}", failures[0]),
        };
        toast(state, message);
    }
    landed
}

/// Copy one batch into the store, answering with the stored address per book id.
pub(super) async fn copy_batch(
    state: AppState,
    task: &str,
    pending: &[(String, &FoundFile)],
) -> Result<HashMap<String, String>, String> {
    let requests: Vec<StoreRequest> = pending
        .iter()
        .map(|(book_id, file)| StoreRequest {
            path: file.path.clone(),
            id: book_id.clone(),
        })
        .collect();
    let results = wire::store_books(task, &requests).await?;
    Ok(partition_store_results(state, results, "files"))
}

/// Measure a batch of store copies in one pass: stored address to its
/// fingerprint. A copy that cannot be measured is simply absent, and the row
/// it belongs to keeps a pending flag the startup sweep finishes.
pub(super) async fn measure_stores(stores: Vec<String>) -> HashMap<String, Fingerprint> {
    if stores.is_empty() {
        return HashMap::new();
    }
    wire::verify_paths(stores)
        .await
        .ok()
        .map(|checks| {
            checks
                .into_iter()
                .filter_map(|check| Some((check.path.clone(), check.fingerprint()?)))
                .collect()
        })
        .unwrap_or_default()
}
