//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`   — the data model (modes + [`Appearance`]), the persisted schema
//!   * `tint`    — colour maths: filter pipeline, blend, tinted UI tokens
//!   * `filter`  — the canvas filter as structured maths: per-token matrices,
//!     composition, and the raster pixel loop
//!   * `preview` — the preset-thumbnail preview style/class

mod filter;
mod model;
mod preview;
mod tint;

pub use filter::{bake_pixels, compose_filter_string, FilterMatrix};
pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
