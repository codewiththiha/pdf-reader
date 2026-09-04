//! Dim mode as a transform over the light palette — the text counterpart of
//! the dim pipeline the PDF raster runs.
//!
//! The PDF path applies `brightness(0.8) saturate(0.75) contrast(0.9)` to
//! the raster and composites it with `soft-light` over the dim backdrop
//! (Dim's own `--base-paper`). Those operations are defined per channel in
//! gamma sRGB, so the text path evaluates the SAME chain directly on each
//! token's colour instead of approximating it in OKLCH: a dim text page
//! lands on the same grey a dim PDF page shows.
//!
//! Ink cannot ride the chain blindly — the raster dims black ink to black,
//! which is illegible on the dimmed page — so the ink is re-derived from
//! the dimmed paper by the contrast guard instead (see
//! [`super::contrast`]): on the dark dimmed page the ink goes light, capped
//! just below white.

use crate::appearance::base::base_tokens;
use crate::appearance::shared::oklch::{hex_to_oklch, hex_to_srgb, oklch_css, srgb_to_hex};
use crate::appearance::BaseMode;
use crate::appearance_text::contrast::{ensure_contrast, MIN_TEXT_CONTRAST};

/// Dim one page colour the way the raster pipeline dims the PDF page:
/// `brightness(0.8)` -> `saturate(0.75)` -> `contrast(0.9)` per channel in
/// sRGB, then `soft-light` over the dim backdrop. The backdrop is Dim's
/// own `--base-paper`, the colour the canvas blends against — read from
/// the base table so the two can never drift apart.
pub fn apply_dim(hex: &str) -> String {
    let Some((r, g, b)) = hex_to_srgb(hex) else {
        return hex.to_string();
    };

    // brightness(0.8)
    let (r, g, b) = (r * 0.8, g * 0.8, b * 0.8);
    // saturate(0.75)
    let (r, g, b) = saturate(r, g, b);
    // contrast(0.9): c' = c * 0.9 + 0.05
    let (r, g, b) = (r * 0.9 + 0.05, g * 0.9 + 0.05, b * 0.9 + 0.05);

    let Some((br, bg, bb)) = hex_to_srgb(base_tokens(BaseMode::Dim).paper) else {
        return hex.to_string();
    };
    srgb_to_hex(soft_light(br, r), soft_light(bg, g), soft_light(bb, b))
}

/// Re-derive the ink after the paper was dimmed, so the dim page keeps
/// reading: a dimmed paper that went dark needs light ink (the same move a
/// readable dark theme makes), a paper that stayed light only wants its ink
/// dimmed along with it. The contrast guard then pushes the ink away from
/// the paper until [`MIN_TEXT_CONTRAST`] holds — on the dark dimmed page
/// that converges on the 0.95 ceiling. Hue and chroma of the original ink
/// survive, so the ink keeps its tone instead of going paper-mixed.
pub fn apply_dim_text(ink_hex: &str, dimmed_paper_hex: &str) -> String {
    let Some((il, ic, ih)) = hex_to_oklch(ink_hex) else {
        return ink_hex.to_string();
    };
    let Some((pl, _, _)) = hex_to_oklch(dimmed_paper_hex) else {
        return ink_hex.to_string();
    };

    let target_l = if pl < 0.5 {
        (pl + 0.55).min(0.95)
    } else {
        il * 0.85
    };
    let l = ensure_contrast(target_l, pl, MIN_TEXT_CONTRAST).min(0.95);
    oklch_css(l, ic, ih)
}

/// The `saturate(0.75)` step: scale the HSL saturation of the sRGB triple
/// by 0.75, hue and lightness held. With hue fixed, every channel's
/// distance from the HSL midpoint scales with the max-min span, so the
/// triple maps back by one factor.
fn saturate(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == min {
        // Achromatic: saturation is undefined, and the filter is identity.
        return (r, g, b);
    }
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let d2 = s * 0.75 * (1.0 - (2.0 * l - 1.0).abs());
    let f = d2 / d;
    (l + (r - l) * f, l + (g - l) * f, l + (b - l) * f)
}

/// One step of `mix-blend-mode: soft-light` over the dim backdrop, per the
/// W3C formula: `cb` is the backdrop channel, `cs` the source.
fn soft_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            cb.sqrt()
        };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (L, C, H) parsed out of an `oklch(...)` literal.
    fn lch(value: &str) -> (f64, f64, f64) {
        let inner = value.trim_start_matches("oklch(").trim_end_matches(')');
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().unwrap());
        (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn white_paper_lands_on_the_dim_grey() {
        // The whole point: a dim text page must show the same grey the dim
        // PDF pipeline produces, so the two formats read as one look.
        assert_eq!(apply_dim("#ffffff"), "#35383d");
    }

    #[test]
    fn neutrals_map_onto_the_dim_grey_with_only_the_backdrops_cast() {
        // A neutral input must gain no real saturation: the only channel
        // spread left is the cool cast of the dim backdrop itself (#1a1c1f),
        // which is exactly the cast a dim PDF page wears.
        assert_eq!(apply_dim("#808080"), "#16181a");
    }

    #[test]
    fn malformed_input_is_passed_through_untouched() {
        assert_eq!(apply_dim("nonsense"), "nonsense");
        assert_eq!(apply_dim_text("nonsense", "#35383d"), "nonsense");
        assert_eq!(apply_dim_text("#1f2937", "nonsense"), "#1f2937");
    }

    #[test]
    fn the_dimmed_page_gets_light_readable_ink() {
        let ink = apply_dim_text("#1f2937", "#35383d");
        let (l, c, _) = lch(&ink);
        assert!(l > 0.9, "dim ink must be light, got L={l}");
        assert!(c < 0.05, "ink must stay near-neutral, got C={c}");
        let (pl, _, _) = hex_to_oklch("#35383d").unwrap();
        assert!(crate::appearance_text::contrast::contrast_ratio(l, pl) >= 2.4);
    }

    #[test]
    fn a_still_light_paper_keeps_a_dark_dimmed_ink() {
        // The light-paper branch: the ink dims with the page and the guard
        // holds it to the floor rather than darkening it past the target.
        let ink = apply_dim_text("#1f2937", "#b8b8b8");
        let (l, _, _) = lch(&ink);
        let (pl, _, _) = hex_to_oklch("#b8b8b8").unwrap();
        assert!(l < 0.3, "ink should stay dark, got L={l}");
        assert!(crate::appearance_text::contrast::contrast_ratio(l, pl) >= MIN_TEXT_CONTRAST);
    }
}
