//! The text/Markdown appearance pipeline: page colours derived directly in
//! OKLCH, written as `--tx-*` tokens — no CSS filter involved.
//!
//! A PDF page is an always-light raster, so its appearance works through a
//! CSS filter chain ([`crate::appearance_pdf`]). A text page paints real
//! DOM type, so its paper and ink can be computed once and assigned
//! outright; this module is that computation, and it is DELIBERATELY not
//! the PDF maths — a text page wants a bright light paper in Light mode
//! (wherever the tint slider sits), a darkish grey paper in Dark, and a
//! medium-dark grey paper with dark ink in Dim, with the ink mostly black
//! on bright paper, mostly white on dark paper, and always carrying a
//! whisper of the paper's hue.
//!
//!   * [`palette`] — [`palette::TextPalette::compute`]: the per-mode
//!     lightness anchors + the tint
//!   * [`preview`] — the preset swatch rendered in the text palette
//!   * [`tokens`]  — the `--tx-*` CSS variables the palette flattens to

pub mod palette;
pub mod preview;
pub mod tokens;
