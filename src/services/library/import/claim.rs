//! One walk per root: the synchronous claim that keeps two runs off one
//! folder's ledger, the ask each claim is answering, and the drop guard that
//! releases it however a run ends — handing the root straight to an explicit
//! ask that arrived while a rescan was walking it.

use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::services::library::{folder_label, toast};
use crate::state::AppState;

use super::Asked;

thread_local! {
    /// The roots a folder run is walking right now, and the ask each one is
    /// answering. One run per root, claimed synchronously and released when the
    /// run's future drops — see [`claim_root`].
    static RUNNING: RefCell<HashMap<String, Asked>> = RefCell::new(HashMap::new());
    /// The explicit run waiting for a root a rescan is walking: one start per
    /// root, run by the rescan's own release rather than polled for, so an ask
    /// is never refused by a walk the app started for itself and never starts
    /// one tick before the ledger it needs is written back.
    static WAITING: RefCell<HashMap<String, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
}

/// A claimed root, released when the run's future ends however it ends — a
/// completion, a failure or a panic all drop the guard the same way.
pub(super) struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
        // Taken out and called with no borrow outstanding on either map: the
        // start it hands over claims this very root, and a claim made inside a
        // borrow of the map it writes is a borrow the RefCell refuses.
        let start = WAITING.with(|waiting| waiting.borrow_mut().remove(&self.0));
        if let Some(start) = start {
            start();
        }
    }
}

/// The sentence a second ask for a folder that is already being walked gets.
///
/// One spelling, because the two doors that can refuse a run — an import and a
/// replace — refuse it for the same reason and owe the reader the same words. A
/// toast each door worded itself would eventually differ about whether the
/// refusal was about this folder or about imports generally.
///
/// A rescan in flight is NOT a refusal and never reaches this sentence: the
/// reader's ask waits for it instead, which is [`when_root_is_free`]'s whole
/// job. What is left to refuse is a second ask the reader made themselves.
pub(super) fn already_importing(state: AppState, root: &str) {
    toast(
        state,
        format!("{} is already being imported.", folder_label(root)),
    );
}

/// The ask a run in flight is answering, if one owns this root.
fn holder(root: &str) -> Option<Asked> {
    RUNNING.with(|running| running.borrow().get(root).copied())
}

/// Whether a run owns this root or is queued to: the question a fold asks before
/// it moves a tree's shelves, where a run that has not started walking yet is as
/// much a run about to write the ledger as one that has.
pub(super) fn root_is_claimed(root: &str) -> bool {
    holder(root).is_some()
        || WAITING.with(|waiting| waiting.borrow().contains_key(root))
}

/// Claim `root` for one run, or answer `None` when one is already in flight.
///
/// Two concurrent walks of one folder are two snapshots of the same ledger row
/// and two writes back to it, and the second write drops whatever the first
/// run placed — a `placed` set that lost an entry re-adds a book the reader
/// already filed, and a tombstone that lost one resurrects a book they
/// removed. The check-and-claim is one synchronous step (the webview is
/// single-threaded), so two runs started in the same tick cannot both pass it.
///
/// The ask travels with the claim because the release owes an answer to it: a
/// rescan stepping aside for a waiting import is [`when_root_is_free`]'s rule,
/// and the rule cannot be asked of a claim that did not say what it was.
pub(super) fn claim_root(root: &str, asked: Asked) -> Option<RootClaim> {
    let free = RUNNING.with(|running| {
        running.borrow_mut().insert(root.to_string(), asked).is_none()
    });
    free.then(|| RootClaim(root.to_string()))
}

/// Run `start` now if the root is free, or the moment the rescan walking it
/// releases it. Answers `false` when the root belongs to an ask the READER made
/// — a second import, or a replace — which is the refusal the caller owes a
/// sentence for.
///
/// The rule this exists for, and it is the sibling of the import module's "an
/// ask outranks a removal": **an ask outranks a rescan.** A focus rescan is a
/// question the app asked itself, and it is in flight constantly on a watched
/// library — including at the exact moment a picker closes, because a native
/// dialog handing the window back is a focus event like any other. Refusing the
/// reader's import because the app happened to be walking the same folder is a
/// refusal of the one ask that matters, and the shape it wears is a re-import
/// that quietly returns nothing: the rescan honours the tombstones the reader's
/// removals wrote, which is its whole job, and the import that would have
/// lifted them never ran.
///
/// Waiting rather than cancelling, because the rescan's write is the ledger the
/// import needs to read: a run cut short half way through its landing would
/// leave a `placed` set and a shelf map nobody finished, and the import that
/// walked on top of it would answer for a folder that is not the one on disk.
/// The rescan is already nearly done — it is a walk, and the walk is the slow
/// part — and the reader's card is on the dock from the click that queued it.
pub(super) fn when_root_is_free<F>(root: &str, start: F) -> bool
where
    F: FnOnce() + 'static,
{
    match holder(root) {
        None => {
            start();
            true
        }
        // A rescan holds the root: the ask waits for its release, which the
        // drop guard hands over. One waiter per root, so a second ask while one
        // is queued is the refusal rather than a start that overwrites it.
        Some(Asked::OnFocus) => WAITING.with(|waiting| {
            match waiting.borrow_mut().entry(root.to_string()) {
                Entry::Occupied(_) => false,
                Entry::Vacant(slot) => {
                    slot.insert(Box::new(start));
                    true
                }
            }
        }),
        Some(Asked::Explicitly) => false,
    }
}
