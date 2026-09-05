//! PDF-specific domain logic: the page frame, its outline, its search index
//! and the grid its rasters are snapped to.
//!
//! This is the smallest crate in the reader on purpose. Everything that a plain
//! text or Markdown document shares — the settings model, the appearance and
//! tint pipeline, the zoom ladder, the view modes and the spread arithmetic, the
//! floating-box geometry, the search result shape, the chapter node — lives in
//! `reader-core`, because a format that is not PDF needs it too and used to have
//! to reach through this crate's name to say so. What stays here is only what is
//! meaningless without a page of PDF on screen.
//!
//! Pure computation, as before: no wasm and no DOM beyond the one device-pixel
//! read at the presentation boundary, and unit-testable on the host via
//! `cargo test -p pdf-core`.

pub mod outline;
pub mod pixel_grid;
pub mod search;

/// Height of the glass toolbar, in CSS px. The viewer scrollport spans the
/// full window height and no view reserves this band: the bar is an overlay
/// that reveals on hover, so the pages start at the very top and travel under
/// it. It remains the reference for anything that must clear the bar while it
/// IS shown (the search reveal's dead zone, the traffic-light centring).
///
/// MUST stay in sync with Tailwind `h-12` on the title bar.
///
/// The one constant that only means something once there is a page of paper
/// on screen — the band the raster has to clear. Everything the four view
/// modes share (the mode enum, the axis, the page gap, the render budget, the
/// spread arithmetic and the rescale anchor) is format-agnostic and lives in
/// `reader_core::view`, because a plain-text document is scrolled, spread and
/// zoomed through the very same model. It sits at the crate root rather than
/// in a module of its own because a module per constant is overhead the next
/// reader pays for; when the page frame grows a second constant, promote it
/// back into a `layout` module.
pub const TOOLBAR_H: f64 = 48.0;
