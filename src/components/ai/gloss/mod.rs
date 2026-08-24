//! Gloss word-card: a spring-driven morph from a selection bloom into an
//! explanation card, ported from the Gloss reference.
//!
//! The three separable pieces that crossed over are the only things that did:
//! the geometry + spring math (pure, in `pdf_core::gloss`), the anchor
//! tracking (the PDF-specific replacement for the reference's `<mark>`), and
//! the scroll-to-close capture-phase trick. The warm theme, Fraunces/
//! Newsreader and the article/mark wrapping all stayed behind.
//!
//! Layout:
//! * [`spring`] — the spring as a Leptos effect (the rAF loop).
//! * [`anchor`] — capturing the selection as a page-space rect and
//!   re-projecting it onto the screen.
//! * [`marks`]  — the persistent highlight layer painted inside each page.
//! * [`util`]   — viewport/scroll helpers, capture-phase listener, scroller
//!   edge guards, reduced-motion.
//! * [`surface`]— the morphing surface component.
//! * [`popover`]— the orchestrating state machine (replaces `AiPopover`).

pub mod anchor;
pub mod marks;
pub mod popover;
pub mod spring;
pub mod surface;
pub mod util;
