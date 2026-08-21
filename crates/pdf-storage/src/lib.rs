//! Key-value persistence abstraction.
//!
//! The app talks to [`PdfStorage`], never to a concrete backend. The only
//! live impl is [`local::LocalStorage`] (what the app always used, reached
//! through `window.localStorage`); switching backends is a one-line change at
//! startup.

pub mod local;

pub use local::LocalStorage;

/// Minimal key-value store for persisted app state.
///
/// `Send + Sync` so a backend can live behind a reference from any thread;
/// the wasm impls are trivially safe.
pub trait PdfStorage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn remove(&self, key: &str);
}
