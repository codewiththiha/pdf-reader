//! The persistence backend: [`PdfStorage`] + its localStorage impl.
//!
//! The app talks to [`PdfStorage`], never to a concrete backend. The only
//! live impl is [`LocalStorage`] (what the app always used, reached through
//! `window.localStorage`); swapping in another backend is a one-line change
//! at startup.

/// Minimal key-value store for persisted app state.
///
/// `Send + Sync` so a backend can live behind a reference from any thread;
/// the wasm impls are trivially safe.
pub trait PdfStorage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}

/// localStorage-backed [`PdfStorage`]. Identical behaviour to the original
/// `window.PDFReader.storageGet/storageSet` wrappers, minus the indirection.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStorage;

impl PdfStorage for LocalStorage {
    fn get(&self, key: &str) -> Option<String> {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(key)
            .ok()?
    }

    fn set(&self, key: &str, value: &str) {
        if let Some(Some(storage)) = web_sys::window().and_then(|w| w.local_storage().ok()) {
            let _ = storage.set_item(key, value);
        }
    }
}
