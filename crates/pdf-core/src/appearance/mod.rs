//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`   — the data model (modes + [`Appearance`]), the persisted schema
//!   * `filter`  — the canvas filter pipeline as numbers (the definition of record)
//!   * `tint`    — colour maths: filter pipeline, blend, tinted UI tokens
//!   * `preview` — the preset-thumbnail preview style/class

mod filter;
mod model;
mod preview;
mod tint;

pub use filter::{FilterMatrix, FilterOp};
pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
