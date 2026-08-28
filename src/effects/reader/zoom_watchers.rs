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
//! Both are DEBOUNCED. The sidebar slides its width over 300ms and a window
//! drag reports a new container size every frame, so both stream container
//! widths at the reader. A trailing debounce swallows the burst: during the
//! slide every frame re-postpones the fire, so the refit lands once the
//! transition has settled and is always measured against the finished width
//! rather than a half-open window mid-slide.
//!
//! The fire is deliberately UNTWEENED. A refit that tracks a live resize has
//! to land in the frame it was asked for; queueing a 120ms animation against
//! each new width had the page visibly chasing the window. Manual zooms keep
//! their tween — they are a single gesture, not a stream.
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
use crate::state::reader::ZoomCommand;
use crate::state::{AppState, ReaderState, SidebarMode};

/// Trailing debounce for a refit or a window constraint. Long enough that the
/// burst of container widths a sidebar slide or a window drag emits is
/// postponed on every frame and the fire lands once on the finished width;
/// short enough that a resize still feels immediate.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(50);

/// Must be called once from the reader shell (ReaderPage).
pub fn fit_watcher(state: ReaderState, sidebar: RwSignal<SidebarMode>) {
    // Built in the owner that calls us, not inside the effect: the debouncer
    // is one per watcher and disarms itself on cleanup, so a fire cannot
    // land on a reader that has already been disposed.
    let refit = use_debounce(WATCH_DEBOUNCE, move || {
        // Untweened: a fit tracks the window, so it must land in the frame it
        // was resolved in rather than chase it for the length of a tween.
        state.viewer.zoom.post(ZoomCommand::Refit, false);
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
        state.reader.viewer.zoom.post(ZoomCommand::Constrain, false);
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
