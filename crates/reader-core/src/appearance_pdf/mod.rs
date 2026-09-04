//! The PDF appearance pipeline: the canvas filter chain, the blend mode and
//! the tinted UI-token overrides.
//!
//! PDF-specific by construction. A PDF page is an always-light raster, so
//! base mode and tint reach it through CSS filters over the bitmap, and the
//! seven `--color-*` UI tokens are overridden from the same maths. Text and
//! Markdown pages do not import this module — they derive their own palette
//! directly (see `appearance_text`). The shared kernel both pipelines build
//! on lives in [`crate::appearance::shared`].
//!
//!   * [`filter`] — `canvas_filter()` / `canvas_blend()`: the raster pipeline
//!   * [`tint`]   — the tinted UI-token overrides that ride along with it

pub mod filter;
pub mod tint;
