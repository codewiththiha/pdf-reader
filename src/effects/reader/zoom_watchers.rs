//! The zoom sources: thin reactive watchers that re-ask the zoom controller
//! when the world under the reader changes.
//!
//! Neither watcher computes a scale, touches a zoom signal, or calls the
//! engine — they POST COMMANDS ([`ZoomCommand`]) and the controller's one
//! transition pipeline does the rest. That is the whole point of the
//! split: fit width, fit page, a sidebar slide, a window resize and a
//! manual `+` all land through the same capture → tween → commit path, so
//! they can no longer race along separate code paths.
//!
//! Both are DEBOUNCED by the sidebar's slide duration. A window resize and a
//! sidebar flex transition both report a new container width on every one of
//! their frames, and resolving against each one meant a dozen relayouts per
//! gesture — each measured against a half-open window, so the fit width
//! (and with it the shrink-to-fit ceiling) came out clipped at whatever
//! intermediate width happened to arrive last. One trailing fire, after the
//! slide has settled, resolves against the finished geometry; and because it
//! is now a single command rather than a storm, it can afford to tween.
//!
//! - `fit_watcher`: while a fit mode is active, any change of window,
//!   view mode, margin, current page or document re-resolves the fit.
//!   A page turn in a mixed-size book legitimately re-fits — through the
//!   same animated transition a zoom button uses.
//! - `resize_watcher`: while the reader has zoomed by hand, a resize
//!   re-resolves `min(desired, fit-width)`, so a narrowed window shrinks
//!   the page without forgetting the zoom the reader chose.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::math::FitMode;

use crate::components::primitives::hooks::use_timeout::use_debounce;
use crate::components::sidebar::shell::SIDEBAR_SLIDE_MS;
use crate::state::reader::ZoomCommand;
use crate::state::{AppState, ReaderState, SidebarMode};

/// One sidebar slide is long enough for every intermediate container width to
/// have arrived, so a fire scheduled at the end of it always measures the
/// finished window. Tied to the aside's own `duration-300` so the two cannot
/// drift apart.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(SIDEBAR_SLIDE_MS);

/// Must be called once from the reader shell (ReaderPage).
pub fn fit_watcher(state: ReaderState, sidebar: RwSignal<SidebarMode>) {
    // Built in the owner that calls us, not inside the effect: the debouncer
    // is one per watcher and disarms itself on cleanup, so a fire cannot
    // land on a reader that has already been disposed.
    let refit = use_debounce(WATCH_DEBOUNCE, move || {
        state.viewer.zoom.post(ZoomCommand::Refit, true);
    });

    Effect::new(move |_| {
        // Every dependency is a tracked read; none of the values are needed
        // locally — the controller re-reads the world when it resolves.
        let fit = state.viewer.fit.get();
        let _ = state.viewer.mode.get();
        let _ = state.viewer.container_size.get();
        let _ = state.viewer.page_margin.get();
        let _ = state.viewer.page.get();
        // Reading the sidebar re-runs the watcher when it opens/closes,
        // because that changes the available container width.
        let _ = sidebar.get();
        if fit == FitMode::None {
            return; // a manual zoom owns the scale; stand down entirely
        }
        if state.viewer.zooming_now() {
            return; // a zoom is mid-flight; let it settle first
        }
        // Postpones any pending fire and schedules one at the end of the
        // current burst of container sizes.
        refit.trigger();
    });
}

/// Must be called once from the reader shell (ReaderPage), alongside
/// `fit_watcher`.
pub fn resize_watcher(state: AppState) {
    let constrain = use_debounce(WATCH_DEBOUNCE, move || {
        state.reader.viewer.zoom.post(ZoomCommand::Constrain, true);
    });

    Effect::new(move |_| {
        // Only when the reader asked for the shrink-to-fit behaviour.
        if !state.settings.with(|s| s.layout.constrain_zoom_to_window) {
            return;
        }
        // Only when the reader zoomed by hand: while a fit mode is active,
        // the fit watcher owns resizes.
        if state.reader.viewer.fit.get() != FitMode::None {
            return;
        }
        if state.reader.viewer.zooming_now() {
            return; // a zoom is mid-flight; let it settle first
        }
        let _ = state.reader.viewer.container_size.get();
        let _ = state.reader.viewer.page_margin.get();
        let _ = state.reader.viewer.page.get();
        constrain.trigger();
    });
}
