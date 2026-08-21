//! The collision-aware toolbar: entries that don't fit move into the "…"
//! overflow popover. This is application chrome (it measures the app's
//! toolbar elements), not a generic UI primitive.

pub mod adaptive_group;

pub use adaptive_group::{AdaptiveGroup, OverflowRow, ToolbarEntry};
