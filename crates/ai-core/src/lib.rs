//! The format-agnostic core of the AI reading features.
//!
//! Everything here is independent of any document format: the wire types of
//! the word-explanation backend ([`types`]), the gloss card's geometry and
//! spring ([`gloss`], [`spring`]), and the Tauri `explain_word` kickoff
//! ([`bridge`]).
//!
//! What this crate DOES depend on is `reader-core`, and only for things the
//! reader owns rather than the AI: the word card's *settings* are flat `gloss_*`
//! fields of the persisted `Settings` blob (`reader_core::settings`, so
//! `GlossColor` and `GlossDensity` live there — a crate whose types the schema
//! names cannot also own part of the schema), and the card's spring is
//! `reader_core::spring`, which the floating panels step too. Both directions
//! point the same way: features may lean on the reader, never the reverse.
//!
//! The dependency rule is one-way: format crates (pdf-core, the app) depend
//! on this crate, never the reverse — so a future format (epub, a reflowable one,
//! ...) reuses the wire protocol, the card, the gloss mark trait and the springs
//! without touching anything here, and only implements
//! [`gloss::mark::MarkAnchor`] for its own notion of "where in the document".
//!
//! The pure modules are unit-testable on the host via
//! `cargo test -p ai-core`.

pub mod bridge;
pub mod gloss;
pub mod types;
