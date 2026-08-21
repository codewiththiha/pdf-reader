//! App-level state: the viewer pieces come from `pdf_viewer::state`; this adds
//! the app chrome (settings, library, covers, toast). Field paths match the
//! viewer state exactly, so components work unchanged in either context.

use std::collections::HashMap;

use leptos::prelude::RwSignal;

use crate::state::library::{CoverImage, RecentBook};
use pdf_core::settings::Settings;
use pdf_viewer::{DocumentState, SearchState, SidebarMode, ViewerSignals, ViewerState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    #[allow(dead_code)] // reserved: info toasts (only errors are emitted so far)
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
}

/// UI chrome state: the sidebar and the toast surface. Grouped so new UI
/// signals don't keep piling onto the flat `AppState` struct.
#[derive(Clone, Copy)]
pub struct UiState {
    /// Which sidebar panel (if any) is open.
    pub sidebar: RwSignal<SidebarMode>,
    /// Current toast (if any), rendered by the app-root `ToastHost`.
    pub toast: RwSignal<Option<Toast>>,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: RwSignal<Settings>,
    pub doc: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ui: UiState,
    /// The "recent books" library: recently opened documents, most-recent
    /// first, each carrying its last-reached page.
    pub library: RwSignal<Vec<RecentBook>>,
    /// Cover art (page-1 JPEG data URLs) keyed by path.
    pub covers: RwSignal<HashMap<String, CoverImage>>,
}

impl AppState {
    /// The viewer slice of app state. Field paths match `ViewerState` exactly,
    /// so reusable viewer components accept this without copying atoms.
    pub fn viewer_state(self) -> ViewerState {
        ViewerState::new(self.doc, self.viewer, self.search, self.ui.sidebar)
    }
}

impl From<AppState> for ViewerState {
    fn from(state: AppState) -> Self {
        state.viewer_state()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: RwSignal::new(Settings::default()),
            doc: DocumentState::default(),
            viewer: ViewerSignals::default(),
            search: SearchState::default(),
            ui: UiState {
                sidebar: RwSignal::new(SidebarMode::None),
                toast: RwSignal::new(None),
            },
            library: RwSignal::new(Vec::new()),
            covers: RwSignal::new(HashMap::new()),
        }
    }
}
