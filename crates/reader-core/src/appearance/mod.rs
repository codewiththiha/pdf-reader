//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`   — the data model (modes + [`Appearance`]), the persisted schema
//!   * `base`    — the raw palettes behind each base mode
//!   * `shared`  — the kernel both pipelines consume: the OKLCH maths, the
//!     tint hue mapping and ceilings, the noise/texture helpers
//!   * `preview` — the preset-thumbnail preview style/class
//!
//! The two pipelines live beside this kernel as their own crate-level
//! modules: `appearance_raster` (the filter chain + UI-token overrides for
//! pages that arrive as bitmaps) and `appearance_reflowable` (the
//! direct-colour palette for pages painted as CSS text). Both read the model
//! and the shared kernel; neither reads the other.

pub mod base;
mod model;
pub(crate) mod preview;
pub mod shared;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
