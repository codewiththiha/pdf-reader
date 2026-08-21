//! Application bootstrap: the storage backend, the persisted state
//! (settings, library, covers), and the contexts the pages read (app
//! state, the viewer state slice, the page texture signal).

use leptos::prelude::*;

use pdf_core::appearance::TextureMode;
use pdf_viewer::TextureSignal;
use crate::state::AppState;
use crate::storage::{init_storage, load_covers, load_library, load_settings};

/// Install the storage backend. localStorage today; swapping for another
/// `PdfStorage` backend is a one-line change here.
pub(crate) fn install_storage() {
    init_storage(Box::new(pdf_storage::LocalStorage));
}

/// App state seeded from the persisted settings/library/covers.
pub(crate) fn create_app_state() -> AppState {
    AppState {
        settings: RwSignal::new(load_settings()),
        library: RwSignal::new(load_library()),
        covers: RwSignal::new(load_covers()),
        ..AppState::default()
    }
}

/// Provide the app-level contexts: the app state (done by the caller),
/// the viewer slice of it, and the texture signal the page hosts need
/// (derived from settings; the viewer never touches settings itself).
pub(crate) fn provide_app_contexts(state: AppState) {
    let texture = RwSignal::new(TextureMode::None);
    Effect::new(move || {
        let t = state.settings.get().appearance.texture;
        texture.set(t);
    });
    provide_context(state.viewer_state());
    provide_context(texture as TextureSignal);
}
