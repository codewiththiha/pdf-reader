//! The text/Markdown appearance pipeline: page colours derived directly in
//! OKLCH, written as `--tx-*` tokens — no CSS filter involved.
//!
//! A PDF page is an always-light raster, so its appearance works through a
//! CSS filter chain ([`crate::appearance_pdf`]). A text page paints real
//! DOM type, so its paper and ink can be computed once and assigned
//! outright; this module is that computation. It mirrors the PDF
//! pipeline's numbers — the same hue mapping, the same per-token chroma
//! ceilings, dim applied as a transform over the light palette — so the
//! two formats stay in visual lockstep without sharing a pipeline.
//!
//!   * [`palette`]  — [`palette::TextPalette::compute`]: base -> dim -> tint
//!   * [`dim`]      — the dim transform: the PDF filter chain evaluated
//!     per colour, so a dim text page lands on the same grey a dim PDF
//!     page shows
//!   * [`contrast`] — the ink/paper contrast guard dim leans on
//!   * [`tokens`]   — the `--tx-*` CSS variables the palette flattens to

pub mod contrast;
pub mod dim;
pub mod palette;
pub mod tokens;
