//! One walk per root: the synchronous claim that keeps two runs off one
//! folder's ledger, and the drop guard that releases it however a run ends.

use std::cell::RefCell;
use std::collections::HashSet;

use crate::services::library::{folder_label, toast};
use crate::state::AppState;

thread_local! {
    /// The roots a folder run is currently walking. One run per root, claimed
    /// synchronously and released when the run's future drops — see
    /// [`claim_root`].
    static RUNNING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// A claimed root, released when the run's future ends however it ends — a
/// completion, a failure or a panic all drop the guard the same way.
pub(super) struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
    }
}

/// The sentence a second ask for a folder that is already being walked gets.
///
/// One spelling, because the two doors that can refuse a run — an import and a
/// replace — refuse it for the same reason and owe the reader the same
/// words. A toast each door worded itself would eventually differ about
/// whether the refusal was about this folder or about imports generally.
pub(super) fn already_importing(state: AppState, root: &str) {
    toast(
        state,
        format!("{} is already being imported.", folder_label(root)),
    );
}

/// Whether a walk of `root` is already in flight — the question the replace
/// answer has to ask BEFORE its purge, because a removal behind a refused
/// claim would be a sweep with no import to answer it.
pub(super) fn root_is_claimed(root: &str) -> bool {
    RUNNING.with(|running| running.borrow().contains(root))
}

/// Claim `root` for one run, or answer `None` when one is already in flight.
///
/// Two concurrent walks of one folder are two snapshots of the same ledger row
/// and two writes back to it, and the second write drops whatever the first
/// run placed — a `placed` set that lost an entry re-adds a book the reader
/// already filed, and a tombstone that lost one resurrects a book they
/// removed. The check-and-claim is one synchronous step (the webview is
/// single-threaded), so two runs started in the same tick cannot both pass it.
pub(super) fn claim_root(root: &str) -> Option<RootClaim> {
    RUNNING
        .with(|running| running.borrow_mut().insert(root.to_string()))
        .then(|| RootClaim(root.to_string()))
}
