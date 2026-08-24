//! Gloss word-card: a spring-driven morph from an in-page highlighter stroke
//! into an explanation card and back down onto it, ported from the Gloss
//! reference (with the Marginalia highlighter look for the mark itself).
//!
//! The three separable pieces that crossed over are the only things that did:
//! the geometry + spring math (pure, in `pdf_core::gloss`), the anchor
//! tracking (the PDF-specific replacement for the reference's `<mark>`), and
//! the scroll-to-close capture-phase trick. The warm theme, Fraunces/
//! Newsreader and the article/mark wrapping all stayed behind.
//!
//! Layout:
//! * [`spring`] — the spring as a Leptos effect (the rAF loop), with a
//!   per-word `reset_to` so a new open never flies in from the last card.
//! * Page-aware anchors live in [`crate::components::ai::anchor`] (shared
//!   with the selection Info pill).
//! * [`marks`]  — the persistent highlighter stroke layer painted inside each page.
//! * [`util`]   — viewport helpers, capture-phase listener, reduced-motion.
//! * [`surface`]— the morphing surface component.
//! * [`popover`]— the orchestrating state machine (replaces `AiPopover`).

pub mod marks;
pub mod popover;
pub mod spring;
pub mod surface;
pub mod util;
