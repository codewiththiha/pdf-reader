//! App-lifetime window-state bridge: the live maximized flag.
//!
//! The frameless caption cluster (titlebar/window_controls.rs) shows
//! maximize or restore per the window's REAL state, and that state changes
//! under it by more than its own button: Win+Arrow snapping, drag-to-edge,
//! taskbar restores, double-clicking the drag region. Every one of those
//! resizes the window, so one `tauri://resize` subscription re-asks the
//! question for all of them at once and publishes into
//! `UiState::window_maximized`, which is all the cluster ever reads.
//!
//! Installed from the app root (install_app_effects) like the other
//! app-lifetime bridges, and for the same reason a page-scoped subscription
//! would be wrong: the parked closure must never be dropped while Tauri's
//! JS still holds the handler (see services/tauri_listen.rs), and a page
//! owner dies on every route change. Probes are coalesced — one in flight,
//! further resize events dropped — because an interactive resize fires an
//! event per frame, far faster than the round trip completes, and the
//! first event after a probe lands always carries the freshest geometry.
//!
//! macOS skips the bridge: its windows are not frameless, nothing reads
//! the flag, and there is no reason to spend an IPC per resize there.

use leptos::prelude::*;

use crate::services::platform::uses_frameless_controls;
use crate::state::AppState;

/// Publish the window's maximized state into `state.ui.window_maximized`:
/// once at install, then on every window resize.
pub fn install_window_state_bridge(state: AppState) {
    if !tauri_bridge::has_tauri() || !uses_frameless_controls() {
        return;
    }

    let probing = StoredValue::new_local(false);
    let probe = move || {
        if probing.get_value() {
            return;
        }
        probing.set_value(true);
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            st.ui.window_maximized
                .set(pdf_engine::api::is_window_maximized().await);
            probing.set_value(false);
        });
    };

    // The install-time answer, so the first caption paint is already right.
    probe();

    crate::services::tauri_listen("tauri://resize", move |_ev| probe());
}
