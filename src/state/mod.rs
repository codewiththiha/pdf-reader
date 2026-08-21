//! App-level state: settings, the document/viewer slice, the library, and
//! the UI chrome. Pure domain logic lives in `pdf-core`; the engine bridge
//! in `pdf-engine`; document lifecycle operations in `services`.

pub mod app;
pub mod library;
pub mod viewer;

pub use app::{AppState, Toast, ToastKind};
pub use viewer::{
    DocumentState, SearchState, SidebarMode, TextureSignal, ViewerSignals, ViewerState,
};
