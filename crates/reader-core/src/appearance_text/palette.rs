//! The text-page palette: [`TextPalette::compute`] turns the shared
//! [`Appearance`] into the concrete colours a text/Markdown page paints.
//!
//! WHY NO FILTER. A PDF page is an always-light raster, so base mode and
//! tint reach it through a CSS filter chain. A text page owns its paper
//! and ink outright — the colours can be computed once and assigned, with
//! no compositor filter layer, no blend mode, and no accidental double
//! transform. The numbers mirror the PDF path (same hue mapping, same
//! per-token chroma ceilings, dim applied to the light palette) so the two
//! formats read as one look.
//!
//! THE THREE STEPS.
//!   1. Start from the base palette (Light / Dark — Dim is step 2).
//!   2. Dim: a transform over the LIGHT palette, exactly like the PDF
//!      filter dims an always-light raster — and it runs FIRST, because
//!      the canvas filter lists the base pipeline before the tint, so a
//!      tinted dim page is a tint over dim in both formats.
//!   3. Tint: each token shifts hue toward the tint and lifts chroma,
//!      lightness untouched — the same invariant as the PDF UI tokens.
//!
//! Dim re-derives the ink afterwards so contrast survives (see
//! [`super::dim`] / [`super::contrast`]); a tint barely reaches ink on the
//! PDF path either (its chroma ceiling is 0.02), so the dimmed ink stays
//! untinted and neutral, exactly where the PDF pipeline keeps it.

use crate::appearance::base::base_tokens;
use crate::appearance::shared::tint::{chroma_ceiling, tinted_token, ui_hue_oklch};
use crate::appearance::{Appearance, BaseMode};
use crate::appearance_text::dim;

/// The complete set of page colours for a text/Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPalette {
    /// The page background colour.
    pub paper: String,
    /// The primary text colour.
    pub ink: String,
    /// Secondary / muted text.
    pub muted: String,
    /// Surface (cards, sidebars).
    pub surface: String,
    /// Borders / dividers.
    pub line: String,
    /// Accent (links, marks).
    pub accent: String,
    pub accent_soft: String,
}

impl TextPalette {
    /// Derive the palette for an appearance. Pure — no DOM, no engine.
    pub fn compute(a: &Appearance) -> TextPalette {
        // Dim is not a palette of its own: like the PDF filter it is a
        // transform over the light look, so the derivation starts from the
        // palette whose paper direction matches the light raster's. Dark
        // keeps its own inverted palette — it is a palette, not a dim.
        let source = if a.base == BaseMode::Dim { BaseMode::Light } else { a.base };
        let base = base_tokens(source);

        if a.base == BaseMode::Dim {
            let paper = dim::apply_dim(base.paper);
            TextPalette {
                ink: dim::apply_dim_text(base.ink, &paper),
                muted: tint_pass(a, &dim::apply_dim(base.muted), "muted"),
                surface: tint_pass(a, &dim::apply_dim(base.surface), "surface"),
                line: tint_pass(a, &dim::apply_dim(base.line), "line"),
                paper: tint_pass(a, &paper, "paper"),
                accent: tint_pass(a, &dim::apply_dim(base.accent), "accent"),
                accent_soft: tint_pass(a, &dim::apply_dim(base.accent_soft), "accent-soft"),
            }
        } else {
            TextPalette {
                paper: tint_pass(a, base.paper, "paper"),
                ink: tint_pass(a, base.ink, "ink"),
                muted: tint_pass(a, base.muted, "muted"),
                surface: tint_pass(a, base.surface, "surface"),
                line: tint_pass(a, base.line, "line"),
                accent: tint_pass(a, base.accent, "accent"),
                accent_soft: tint_pass(a, base.accent_soft, "accent-soft"),
            }
        }
    }
}

/// Tint one token when a tint is active, otherwise hand the base hex
/// through untouched — an untinted Light/Dark page stays byte-identical
/// to the base palette.
fn tint_pass(a: &Appearance, hex: &str, token: &str) -> String {
    if !a.has_tint() {
        return hex.to_string();
    }
    let t = a.tint_strength as f64 / 100.0;
    let target_h = ui_hue_oklch(a.tint_hue as f64);
    tinted_token(hex, target_h, t, chroma_ceiling(token)).unwrap_or_else(|| hex.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::base::base_tokens;
    use crate::appearance::shared::oklch::hex_to_oklch;
    use crate::appearance_text::contrast::contrast_ratio;

    fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance { base, tint_hue: hue, tint_strength: strength, ..Default::default() }
    }

    #[test]
    fn an_untinted_light_page_is_byte_identical_to_the_base_palette() {
        let p = TextPalette::compute(&Appearance::default());
        let base = base_tokens(BaseMode::Light);
        assert_eq!(p.paper, base.paper);
        assert_eq!(p.ink, base.ink);
        assert_eq!(p.muted, base.muted);
        assert_eq!(p.surface, base.surface);
        assert_eq!(p.line, base.line);
        assert_eq!(p.accent, base.accent);
        assert_eq!(p.accent_soft, base.accent_soft);
    }

    #[test]
    fn an_untinted_dark_page_is_byte_identical_to_the_base_palette() {
        let p = TextPalette::compute(&tinted(BaseMode::Dark, 0, 0));
        let base = base_tokens(BaseMode::Dark);
        assert_eq!(p.paper, base.paper);
        assert_eq!(p.ink, base.ink);
        assert_eq!(p.accent, base.accent);
    }

    #[test]
    fn a_tinted_page_matches_the_pdf_token_path_token_for_token() {
        // THE parity invariant: the text palette and the PDF UI overrides
        // share the hue mapping, the ceiling table and the rotation, so a
        // text page's paper IS the chrome's paper at every slider position.
        for (base, hue, strength) in [
            (BaseMode::Light, 104u16, 50u8),
            (BaseMode::Light, 350, 100),
            (BaseMode::Dark, 104, 80),
        ] {
            let a = tinted(base, hue, strength);
            let p = TextPalette::compute(&a);
            let o = a.ui_overrides();
            let want = |name: &str| o.iter().find(|(k, _)| *k == name).unwrap().1.clone();
            assert_eq!(p.paper, want("--color-paper"), "paper @ {hue}/{strength}");
            assert_eq!(p.ink, want("--color-ink"), "ink @ {hue}/{strength}");
            assert_eq!(p.muted, want("--color-muted"), "muted @ {hue}/{strength}");
            assert_eq!(p.surface, want("--color-surface"), "surface @ {hue}/{strength}");
            assert_eq!(p.line, want("--color-line"), "line @ {hue}/{strength}");
            assert_eq!(p.accent, want("--color-accent"), "accent @ {hue}/{strength}");
            assert_eq!(
                p.accent_soft,
                want("--color-accent-soft"),
                "accent-soft @ {hue}/{strength}"
            );
        }
    }

    #[test]
    fn the_lightness_ladder_survives_a_full_strength_tint() {
        // Page brighter than its cards, cards brighter than their borders —
        // the hierarchy the whole UI depends on. Tinted values are emitted
        // as oklch literals, so parse them back.
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let paper = lch(&p.paper).0;
        let surface = lch(&p.surface).0;
        let line = lch(&p.line).0;
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");
    }

    /// (L, C, H) parsed out of an `oklch(...)` literal.
    fn lch(value: &str) -> (f64, f64, f64) {
        let inner = value.trim_start_matches("oklch(").trim_end_matches(')');
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().unwrap());
        (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn dim_is_a_transform_of_the_light_palette_not_a_palette_of_its_own() {
        let p = TextPalette::compute(&tinted(BaseMode::Dim, 0, 0));
        // The dimmed light paper: the same grey the PDF pipeline shows.
        assert_eq!(p.paper, crate::appearance_text::dim::apply_dim("#ffffff"));
        // NOT the dim chrome palette, and not the dark paper either.
        assert_ne!(p.paper, base_tokens(BaseMode::Dim).paper);
        assert_ne!(p.paper, base_tokens(BaseMode::Dark).paper);
        // Darker than light paper, lighter than the dark paper the chrome uses.
        let lp = hex_to_oklch(base_tokens(BaseMode::Light).paper).unwrap().0;
        let dp = hex_to_oklch(base_tokens(BaseMode::Dark).paper).unwrap().0;
        let pl = hex_to_oklch(&p.paper).unwrap().0;
        assert!(pl < lp, "dim paper {pl} must sit below light paper {lp}");
        assert!(pl > dp, "dim paper {pl} must sit above dark paper {dp}");
    }

    #[test]
    fn dim_and_tint_compose_in_the_filters_own_order() {
        // The canvas filter lists the base dim pipeline BEFORE the tint, so
        // a tinted dim text page is the tint applied to the dimmed colours.
        let dim_tinted = tinted(BaseMode::Dim, 104, 50);
        let p = TextPalette::compute(&dim_tinted);
        let target_h = ui_hue_oklch(104.0);
        let want = tinted_token(
            &crate::appearance_text::dim::apply_dim("#ffffff"),
            target_h,
            0.5,
            chroma_ceiling("paper"),
        )
        .unwrap();
        assert_eq!(p.paper, want);
    }

    #[test]
    fn dim_ink_reads_on_the_dimmed_page() {
        let p = TextPalette::compute(&tinted(BaseMode::Dim, 0, 0));
        let ink = lch(&p.ink).0;
        let paper = hex_to_oklch(&p.paper).unwrap().0;
        assert!(ink > 0.9, "dim ink must be light, got L={ink}");
        assert!(contrast_ratio(ink, paper) >= 2.4);
    }
}
