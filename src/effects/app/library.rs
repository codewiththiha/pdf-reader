//! The library's app-lifetime wiring: the measurement pass at startup, the rescan
//! of every watched folder when the window comes back, and the sink that folds
//! the shell's progress beats into the dock's task list.
//!
//! All three are installed once at the app root and none of them belongs to a
//! page. The rescans are deliberately narrow about when they run: a filesystem
//! watcher would fire while the reader is copying files INTO the folder, which is
//! the one moment a half-written PDF is most likely to be measured, whereas a
//! rescan on focus answers the question that was actually asked — "next time the
//! app opens or returns to the foreground" — with no watcher dependency and no
//! race against an import in flight.
//!
//! The order between the first two is not a coincidence and is not left to the
//! caller: [`verify_library`] measures every address the library holds and calls
//! [`rescan_watched`] itself when it is done, because diffing a library that is
//! still carrying placeholder fingerprints would add a second copy of every book
//! a watched folder already holds.

use std::sync::atomic::{AtomicU64, Ordering};

// The prelude is what puts `update` and `with_untracked` on a signal: they are
// trait methods, and a file that only names `RwSignal` gets a struct with no
// methods on it.
use leptos::prelude::*;
use wasm_bindgen::JsValue;

use library_core::wire::ImportProgress;

use crate::components::primitives::hooks::use_custom_event::use_typed_event;
use crate::events::IMPORT_PROGRESS_EVENT;
use crate::services::library::{rescan_watched, verify_library};
use crate::state::AppState;

/// The shortest gap between two rescans, in milliseconds. Focus events are not
/// rare: alt-tabbing back and forth would otherwise walk every watched folder
/// once per flick of the switcher, and a walk is a syscall per entry.
const RESCAN_COOLDOWN_MS: u64 = 5_000;

/// When the last rescan started. Relaxed ordering: the webview is
/// single-threaded, so this only has to be a stamp, never a fence.
static LAST_RESCAN: AtomicU64 = AtomicU64::new(0);

/// Install all three. Called once from the app root, after the theme and the AI
/// bridge and before the OS file handoff — a double-clicked book must not land in
/// the middle of the library's first measurement pass.
pub(crate) fn library_effects(state: AppState) {
    install_progress_sink(state);
    verify_library(state);

    if !tauri_bridge::has_tauri() {
        return;
    }
    crate::services::tauri_listen("tauri://focus", move |ev: web_sys::Event| {
        if focused(&ev) {
            rescan_once(state);
        }
    });
}

/// Fold every progress beat into the dock's task list.
///
/// App-lifetime rather than page-lifetime, and that is the whole reason it is
/// here: an import started on the library page is still running after the reader
/// has opened a book, and a listener mounted on the page would stop counting the
/// moment the route flipped. A beat for a task the list does not hold is dropped
/// — which is how a quiet rescan stays quiet, since it raises no card until it
/// has something to put on one.
fn install_progress_sink(state: AppState) {
    use_typed_event::<ImportProgress>(IMPORT_PROGRESS_EVENT, move |beat| {
        state.library.tasks.update(|tasks| {
            if let Some(task) = tasks.iter_mut().find(|t| t.id == beat.task) {
                task.beat(&beat);
            }
        });
    });
}

/// Rescan unless one just ran. The cooldown is the whole guard: a rescan that
/// finds nothing writes nothing, so the only cost of a second one is the walk.
fn rescan_once(state: AppState) {
    let now = js_sys::Date::now() as u64;
    let last = LAST_RESCAN.load(Ordering::Relaxed);
    if now.saturating_sub(last) < RESCAN_COOLDOWN_MS {
        return;
    }
    LAST_RESCAN.store(now, Ordering::Relaxed);
    rescan_watched(state);
}

/// Whether a focus event is a focus and not a blur. Tauri carries the answer in
/// the payload; an event with no readable payload is treated as a focus, because
/// the cost of one extra walk is a directory listing and the cost of ignoring a
/// real one is a shelf that never updates.
fn focused(ev: &web_sys::Event) -> bool {
    let value: &JsValue = ev.as_ref();
    js_sys::Reflect::get(value, &"payload".into())
        .ok()
        .and_then(|payload| payload.as_bool())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::RESCAN_COOLDOWN_MS;
    use crate::state::library::{ImportTask, TaskPhase};
    use library_core::wire::{ImportPhase, ImportProgress};

    fn beat(phase: ImportPhase, done: u32, total: u32) -> ImportProgress {
        ImportProgress {
            task: "t1".into(),
            phase,
            done,
            total,
            name: "dune.pdf".into(),
        }
    }

    #[test]
    fn the_cooldown_is_longer_than_a_walk_and_shorter_than_a_session() {
        // A rescan of a large folder is a few hundred milliseconds of syscalls;
        // five seconds is comfortably above that, and comfortably below the gap a
        // reader leaves between two real returns to the window.
        assert!(RESCAN_COOLDOWN_MS > 500);
        assert!(RESCAN_COOLDOWN_MS < 60_000);
    }

    #[test]
    fn the_two_phases_the_shell_reports_are_the_two_the_dock_draws() {
        // The sink is the only thing that turns a beat into a card, so the
        // mapping is the contract: a scan is indeterminate, a copy is a fraction.
        let mut task = ImportTask::new("t1", "Books");
        task.beat(&beat(ImportPhase::Scan, 300, 0));
        assert_eq!(task.phase, TaskPhase::Scanning);
        assert_eq!(task.fraction(), None);
        task.beat(&beat(ImportPhase::Copy, 4, 8));
        assert_eq!(task.phase, TaskPhase::Copying);
        assert_eq!(task.percent(), Some(50));
        // And the shell never says "done": only the run's owner knows the state
        // write landed, so finishing stays the frontend's call.
        assert!(!task.phase.is_finished());
    }
}
