//! The zoom subsystem: one controller, one transition pipeline.
//!
//! The contract for the whole viewer is that zoom has exactly one owner.
//! Surfaces that want the zoom to change post a `ZoomCommand` through
//! `viewer.zoom.post(...)`; the [`ZoomController`] resolves it against the
//! current window, mode and page, and drives a single
//! [`ZoomTransition`](crate::state::reader::ZoomTransition):
//!
//! ```text
//! capture a logical anchor
//!     ↓
//! resolve the target (manual / fit / window constraint)
//!     ↓
//! tween the VISUAL scale, frame by frame
//!     ↓
//! commit geometry ONCE, restore the anchor, release the freezes
//! ```
//!
//! What deliberately does NOT happen per animation frame:
//!
//! ```text
//! virtualizer.rescale()
//! virtualizer.report_size()
//! scroll_to_index / scroll_to_offset
//! page.set(...)
//! ```
//!
//! Those are transaction-boundary work. The virtualizer keeps its committed
//! geometry for the whole tween (the window cannot churn, the dominant item
//! cannot move, the page number cannot flicker), the pages stretch their
//! existing bitmaps through the display scale, and one commit at the end
//! moves the geometry, the rasters and the scroll position together.
//!
//! Commands travel on a signal rather than a provided context because the
//! keyboard shortcuts are wired at the app root, above the reader's reactive
//! owner, where `use_context` cannot reach — and because a command queue
//! gives the same single-owner guarantee without any process-wide registry.
//! The old thread-local controller registration is gone; `drive` runs for
//! exactly as long as the reader page owns it.

pub mod anchor;
pub mod animation;
pub mod config;
pub mod coordinator;
pub mod target;

pub use coordinator::ZoomController;
