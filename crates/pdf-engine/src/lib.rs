//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Four layers, one job: `bridge` declares the `window.PDFReader` wasm-bindgen
//! externs (the ONLY place they live — typed, so a mount/render call
//! allocates no payload objects), `types` mirrors the engine's return shapes,
//! `api` provides typed `Result`-returning wrappers for the rest of the app
//! (one focused module per surface: document, render, search, paper, dialog,
//! theme), and `paper_session` is the paper session state machine. The `window.__TAURI__` externs this crate still touches (the
//! file dialog) come from the `tauri-bridge` crate, which owns that surface
//! so no format crate does.
//!
//! `bridge` is private: callers go through `api`, except for the raw engine
//! probes the app legitimately needs directly (engine version,
//! `window.PDFReader` presence), which are re-exported here.

mod bridge;

pub mod api;
pub mod paper_session;
pub mod types;

/// Engine probes re-exported at the root. These don't follow the normal
/// `api` path on purpose: they expose the raw engine probe (version,
/// `window.PDFReader` presence) that the app calls directly, even though
/// nothing inside this crate uses them.
pub use bridge::{has_pdf_reader, version};
