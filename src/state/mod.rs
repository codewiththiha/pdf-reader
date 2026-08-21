//! App-level state: settings, the reader slice (document/viewer/search),
//! the library, and the UI chrome. Pure domain logic lives in `pdf-core`;
//! the engine bridge in `pdf-engine`; document lifecycle operations in
//! `services`.

pub mod app;
pub mod library;
pub mod ui;
pub mod viewer;

pub use app::{AppState, Toast, ToastKind};
pub use ui::SidebarMode;
pub use viewer::{ReaderState, TextureSignal};
