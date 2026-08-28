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
//! tween the DISPLAY SCALE, frame by frame
//!     ↓
//! commit the geometry and the render scale, release the freezes
//! ```
//!
//! How the visual change reaches the screen depends on the view mode, and
//! that split is deliberate — the two scroll modes are laid out by different
//! machinery, so the zoom that is smooth in one is a fight in the other:
//!
//! - the HORIZONTAL strip relayouts every frame (`engine::relayout_to`), and
//!   the virtualizer's own `rescale` anchor is what keeps the reader's view
//!   steady while the item sizes underneath it move. Nothing about position
//!   is captured, because there is no seam to hide.
//! - the VERTICAL strip and the PAGINATED modes scale their content surface
//!   through one CSS transform (`zoom::presentation`) for the whole tween,
//!   and the commit replaces that transform with real layout at the same
//!   visual size. They capture a page-centric focus and a stage pivot at
//!   transaction open (`zoom::anchor`) and restore them at the commit, so
//!   the page under the reader's eyes stays on the same screen pixel.
//!
//! What deliberately does NOT happen per animation frame, in either mode:
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
//! issued once at the settled scale, and pages a moving window evicts are
//! bridged briefly by the virtualizer's zombie retention.
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
