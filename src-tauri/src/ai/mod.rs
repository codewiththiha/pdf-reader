//! AI provider wiring for the in-reader dictionary/assistant features.
//!
//! The AI backend lives entirely in this Tauri crate: the WASM frontend
//! cannot spawn the Swift helper process, so it only sends selected text
//! over `invoke` and streams results back. Which provider is compiled is
//! decided at build time:
//!
//!   * Apple Silicon + `ai` feature → the real Apple Intelligence provider
//!     (talks to the on-device Foundation Models framework via `fm-bridge`).
//!   * Everything else (Windows, Linux, Intel Macs, or `ai` disabled) → a
//!     mock provider that fails gracefully.

// Scaffolding: the providers, `WordInfo` and prompts get wired to Tauri
// commands next; until then nothing constructs them and the `AiProvider`
// re-export has no consumer yet.
#![allow(dead_code, unused_imports)]

pub mod prompts;

use serde::{Deserialize, Serialize};

// The data structure we will eventually stream to the frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WordInfo {
    pub word: String,
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

// Apple Silicon Implementation
#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub mod apple;

#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub use apple::AppleAiProvider as AiProvider;

// Fallback Implementation (Windows, Linux, Intel Macs, or 'ai' feature disabled)
#[cfg(not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")))]
pub mod mock;

#[cfg(not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")))]
pub use mock::MockAiProvider as AiProvider;
