//! Generic DOM/timer hooks: each owns one listener/effect family so
//! components read as wiring + view, and the raw pattern (closure parking,
//! cleanup ordering, JS-reference lifetimes) is written once.
//!
//! Contract: nothing here may know what a PDF reader is. These are the
//! layer-1 primitives the floating/interaction systems compose. The one
//! hook that is NOT format-agnostic — the typed CustomEvent hook, which
//! dispatches the app's own event protocol — stays in the app
//! (`primitives::hooks::use_custom_event`).
//!
//! One composite lives here too: [`better_hover`] sits on top of
//! `use_timeout`'s hover primitive and owns the whole auto-hide surface —
//! the shared `hovered` truth, the hold recheck, the pin — so the title bar
//! and the bottom bar (and the next surface) wire one call, not twenty
//! copied lines.

pub mod better_hover;
pub mod dom;
pub mod use_content_size;
pub mod use_raf;
pub mod use_resize_observer;
pub mod use_timeout;
pub mod use_viewport;
pub mod use_window_event;
