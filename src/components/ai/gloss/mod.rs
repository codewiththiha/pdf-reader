//! Gloss word-card: a spring-driven morph from an in-page highlighter stroke
//! into an explanation card and back down onto it, ported from the Gloss
//! reference (with the Marginalia highlighter look for the mark itself).
//!
//! Layout:
//! * The geometry + spring math (pure) lives in `pdf_core::gloss`, including
//!   the side-aware card placement ([`pdf_core::gloss::place_card`]).
//! * Page-aware anchors live in [`crate::components::ai::anchor`] (shared
//!   with the selection Info pill).
//! * [`controller`]     — the state machine hub: signals, open/close, dedup,
//!   remove/restore.
//! * [`placement`]      — the card's target memos (expanded box + drag offset).
//! * [`drag`]           — pointer physics for dragging the expanded card.
//! * [`interactions`]   — window-level behaviour (Escape/outside, exit, flip).
//! * [`hooks`]          — chunk ingestion and content measurement.
//! * [`select_mode`]    — multi-select mode: entry guards, exit paths, the
//!   context-menu listener, the undo pipeline.
//! * [`marks`]          — the persistent highlighter stroke layer per page
//!   (incl. the long-press gesture + contextmenu).
//! * [`select_bar`]     — the bottom-right selection action bar.
//! * [`context_menu`]   — the right-click "Remove highlight" menu.
//! * [`undo_toast`]     — the "Removed n highlights — Undo" toast.
//! * [`spring`]         — the spring as a Leptos effect (the rAF loop).
//! * [`surface`]        — the morphing surface component.
//! * [`popover`]        — wiring + view.

pub mod context_menu;
pub mod controller;
pub mod drag;
pub mod hooks;
pub mod interactions;
pub mod marks;
pub mod placement;
pub mod popover;
pub mod select_bar;
pub mod select_mode;
pub mod spring;
pub mod surface;
pub mod undo_toast;
pub mod util;
