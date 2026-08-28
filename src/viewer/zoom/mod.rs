//! The zoom subsystem: one controller, one transition pipeline.
//!
//! The contract for the whole viewer is that zoom has exactly one owner.
//! Surfaces that want the zoom to change post a `ZoomCommand` through
//! `viewer.zoom.post(...)`; the [`ZoomController`] resolves it against the
//! current window, mode and page, and drives a single
//! [`ZoomTransition`](crate::state::reader::ZoomTransition):
//!
//! ```text
//! resolve the target (manual / fit / window constraint)
//!     ↓
//! open a transition from the scale on screen to that target
//!     ↓
//! tween the DISPLAY SCALE — relaying the layout out through the
//! engine on every frame, so the document resizes continuously
//!     ↓
//! commit the render scale, release the freezes
//! ```
//!
//! The layout IS animated: each frame hands the engine the ratio the display
//! scale just moved through, and the engine's `rescale` anchor is what keeps
//! the reader's view steady while the sizes underneath it change. That is
//! also why nothing about position is captured at transaction open — there
//! is no seam to hide, so there is nothing to restore.
//!
//! What deliberately does NOT happen per animation frame:
//!
//! ```text
//! virtualizer.report_size()
//! scroll_to_index
//! page.set(...)
//! engine.renderPage(...)
//! ```
//!
//! Those are transaction-boundary work. The strips refuse to report measured
//! sizes mid-zoom (the rendered size belongs to a scale that no longer
//! exists), the crisp rasterisation is suspended for the whole tween and
//! issued once at the settled scale, and pages the moving window evicts are
//! bridged briefly by the virtualizer's zombie retention.
//!
//! Commands travel on a signal rather than a provided context because the
//! keyboard shortcuts are wired at the app root, above the reader's reactive
//! owner, where `use_context` cannot reach — and because a command queue
//! gives the same single-owner guarantee without any process-wide registry.
//! The old thread-local controller registration is gone; `drive` runs for
//! exactly as long as the reader page owns it.

pub mod animation;
pub mod config;
pub mod coordinator;
pub mod target;

pub use coordinator::ZoomController;
