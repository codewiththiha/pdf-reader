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
//! - `fit_watcher`: while a fit mode is active, any change of window,
//!   view mode, margin, current page or document re-resolves the fit.
//!   A page turn in a mixed-size book legitimately re-fits — through the
//!   same animated transition a zoom button uses.
//! - `resize_watcher`: while the reader has zoomed by hand, a resize
//!   re-resolves `min(desired, fit-width)`, so a narrowed window shrinks
//!   the page without forgetting the zoom the reader chose.

use leptos::prelude::*;

use pdf_core::math::FitMode;

use crate::state::reader::ZoomCommand;
use crate::state::{AppState, ReaderState, SidebarMode};

/// Must be called once from the reader shell (ReaderPage).
pub fn fit_watcher(state: ReaderState, sidebar: RwSignal<SidebarMode>) {
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
        state.viewer.zoom.post(ZoomCommand::Refit, true);
    });
}

/// Must be called once from the reader shell (ReaderPage), alongside
/// `fit_watcher`.
pub fn resize_watcher(state: AppState) {
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
        let _ = state.reader.viewer.mode.get();
        let _ = state.reader.viewer.container_size.get();
        let _ = state.reader.viewer.page_margin.get();
        let _ = state.reader.viewer.page.get();
        state.reader.viewer.zoom.post(ZoomCommand::Constrain, true);
    });
}
