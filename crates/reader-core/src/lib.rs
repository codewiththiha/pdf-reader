//! The reader's format-agnostic domain: what it can open, how it persists
//! the reader's own choices, and the pure maths every view mode shares.
//!
//! This is the crate that used to be the miscellaneous half of `pdf-core`.
//! The settings model, the appearance/tint pipeline, the theme presets, the
//! zoom ladder, the floating-box geometry, the filename policy, the format
//! registry, the view-mode/spread maths, the search result model and the
//! chapter-node type are all the reader's, not PDF's — a plain-text document
//! is tinted, zoomed, spread and searched through exactly the same code, and
//! naming that shared half after one format is what made every new format
//! start life as a special case.
//!
//! The four axes of the codebase, this crate is axis 1 (pure computation) at
//! its widest: no wasm, no DOM, no leptos, nothing that knows what a page of
//! PDF is. Everything here is unit-testable on the host via
//! `cargo test -p reader-core`.
//!
//! ## What may live here
//!
//! * A type every format needs, or a policy that applies to all of them.
//! * No `Format` branch: the moment a module starts matching on the format it
//!   belongs to that format's crate (`pdf-core`, `txt-core`, `md-core`).
//! * Only two workspace dependencies, both leaves whose types the persisted
//!   schema names: `pdf-paper` (which pixels of a page carry the paper colour)
//!   and `virtual-list` (how far ahead a strip mounts).
//!
//! The spring in [`spring`] is here rather than with the AI features that also
//! use it because the floating panels and the gloss card must ride ONE
//! integrator, and a shared feel cannot be built on a dependency that points
//! from the general code into a feature.

pub mod appearance;
pub mod color;
pub mod filename;
pub mod floating;
pub mod format;
pub mod outline;
pub mod presets;
pub mod search;
pub mod settings;
pub mod spring;
pub mod view;
pub mod zoom_math;

pub use format::{DocumentKind, Format, extensions, first_supported, format_of, is_supported_mime, is_supported_path, SUPPORTED};
pub use outline::OutlineNode;
