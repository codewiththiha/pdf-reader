//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`   — the data model (modes + [`Appearance`]), the persisted schema
//!   * `base`    — the raw palettes behind each base mode
//!   * `shared`  — the kernel both pipelines consume: the OKLCH maths, the
//!     tint hue mapping and ceilings, the noise/texture helpers
//!   * `preview` — the preset-thumbnail preview style/class
//!
//! The two format pipelines live beside this kernel as their own
//! crate-level modules: `appearance_pdf` (the raster filter chain +
//! UI-token overrides) and `appearance_text` (the direct-colour palette
//! for text/Markdown pages). Both read the model and the shared kernel;
//! neither reads the other.

pub mod base;
mod model;
mod preview;
pub mod shared;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
