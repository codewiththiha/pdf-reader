//! One application reporting convention.
//!
//! | Kind                      | Path                                      |
//! |---------------------------|-------------------------------------------|
//! | User-facing failure       | toast / `document.error` (caller's job)   |
//! | Diagnostic-only failure   | [`diagnostic`]                            |
//! | Internal cleanup failure  | ignored (`let _ = …`)                     |
//!
//! This is not a logging framework. It is the one place diagnostic messages
//! go so search, storage, page-canvas and thumbnails don't each invent a
//! `console.log_1` of their own.

use wasm_bindgen::JsValue;

/// Surface a diagnostic-only failure on the console without interrupting the UI.
///
/// Matches [`crate::storage::StorageError::report`]: `console.warn`, tagged
/// with a short scope (`search`, `storage`, `page_canvas`, `thumbnails`).
pub fn diagnostic(scope: &str, detail: impl std::fmt::Display) {
    web_sys::console::warn_1(&JsValue::from_str(&format!("[{scope}] {detail}")));
}
