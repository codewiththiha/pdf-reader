//! App-level state: settings, the document/viewer slice, the library, and
//! the open-document flow. Pure domain logic lives in `pdf-core`; the engine
//! bridge in `pdf-engine`.

pub mod app;
pub mod library;
pub mod open;

pub use app::{AppState, Toast, ToastKind};
