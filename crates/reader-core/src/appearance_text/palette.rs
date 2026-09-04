//! The text-page palette: [`TextPalette::compute`] turns the shared
//! [`Appearance`] into the concrete colours a text/Markdown page paints.
//!
//! WHY ITS OWN MATH. A PDF page is an always-light raster, so base mode and
//! tint reach it through a CSS filter chain (see `appearance_pdf`) whose
//! numbers are chosen for bitmaps. A text page owns its paper and ink
//! outright, so it derives them directly — and deliberately NOT with the
//! PDF maths, because the two formats need different things:
//!
//!   * Light: the paper is BRIGHT (L 0.98) wherever the tint slider sits,
//!     and the ink stays mostly black (L 0.15) so it reads on it.
//!   * Dark: the paper is a DARKISH GREY (L 0.24) — never pitch black —
//!     and the ink mostly white (L 0.92).
//!   * Dim: the paper is a MEDIUM-DARK grey (L 0.40) with dark/black ink
//!     (L 0.12).
//!   * The ink always picks up a whisper of the paper's hue, so the pair
//!     reads as one look instead of black-on-coloured.
//!
//! THE RULES.
//!   1. Every mode anchors the paper's lightness AND the ink's. The tint
//!      moves hue and chroma only — a slider can never turn a light page
//!      murky, a dark page glaring, or a dim page bright.
//!   2. The slider hue is an sRGB angle (that is what the picker paints);
//!      it is mapped into OKLCH before emission, so the paper lands on
//!      the colour the swatch actually shows.
//!   3. The neighbours (surface / line / muted) derive TOWARD the ink, so
//!      the ladder follows the ink's own direction in every mode: Light
//!      and Dim (dark ink) deepen from the paper, Dark (light ink)
//!      lightens from it. Dim therefore reads like a grey Light page, not
//!      a washed-out Dark one.
//!   4. The accent keeps the base family untinted (links stay the
//!      reader's accent), then walks onto the slider's colour as the tint
//!      comes up.

use crate::appearance::base::base_tokens;
use crate::appearance::shared::oklch::{hex_to_oklch, oklch_css};
use crate::appearance::shared::tint::ui_hue_oklch;
use crate::appearance::{Appearance, BaseMode};

/// The complete set of page colours for a text/Markdown document.
#[derive(Debug, Clone, PartialEq)]
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
    /// OKLCH lightness of `paper` — the page's own brightness, which the
    /// texture strokes key their dark/light family off.
    pub paper_l: f64,
    /// OKLCH lightness of `ink`.
    pub ink_l: f64,
}

impl TextPalette {
    /// Derive the palette for an appearance. Pure — no DOM, no engine.
    pub fn compute(a: &Appearance) -> TextPalette {
        let t = a.tint_strength as f64 / 100.0;
        let target_h = ui_hue_oklch(a.tint_hue as f64);

        // The per-mode anchors: paper and ink lightness plus the chroma
        // each may reach at full strength.
        let (paper_l, ink_l, paper_c_max, ink_c_max) = match a.base {
            // Bright paper, mostly-black ink with a whisper of colour.
            BaseMode::Light => (0.98, 0.15, 0.08, 0.03),
            // Darkish grey paper — NOT pitch black — mostly-white ink.
            BaseMode::Dark => (0.24, 0.92, 0.10, 0.04),
            // Medium-dark paper, dark/black ink.
            BaseMode::Dim => (0.40, 0.12, 0.08, 0.03),
        };

        let paper = oklch_css(paper_l, paper_c_max * t, target_h);
        let ink = oklch_css(ink_l, ink_c_max * t, target_h);
        let muted = oklch_css(lerp(paper_l, ink_l, 0.45), ink_c_max * 0.6 * t, target_h);
        let surface = oklch_css(lerp(paper_l, ink_l, 0.06), paper_c_max * 0.8 * t, target_h);
        let line = oklch_css(lerp(paper_l, ink_l, 0.18), paper_c_max * 0.5 * t, target_h);

        // The accent: the base family untinted, the slider's colour once a
        // tint is up — its hue walks from the base accent's (so a 5% tint
        // is still recognisably the reader's accent) and its chroma lifts
        // with strength.
        let (accent_l, accent_soft_l) = if a.base == BaseMode::Light {
            (0.55, 0.92)
        } else {
            (0.75, 0.30)
        };
        let base = base_tokens(a.base);
        let (accent, accent_soft) = if a.has_tint() {
            let (_, _, ah0) = hex_to_oklch(base.accent).unwrap_or((accent_l, 0.12, target_h));
            let mut delta = target_h - ah0;
            while delta > 180.0 {
                delta -= 360.0;
            }
            while delta < -180.0 {
                delta += 360.0;
            }
            let accent_h = (ah0 + delta * t).rem_euclid(360.0);
            (
                oklch_css(accent_l, 0.12 + t * 0.08, accent_h),
                oklch_css(accent_soft_l, 0.04 + t * 0.04, accent_h),
            )
        } else {
            (base.accent.to_string(), base.accent_soft.to_string())
        };

        TextPalette {
            paper,
            ink,
            muted,
            surface,
            line,
            accent,
            accent_soft,
            paper_l,
            ink_l,
        }
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::base::base_tokens;

    fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance { base, tint_hue: hue, tint_strength: strength, ..Default::default() }
    }

    /// (L, C, H) parsed out of an `oklch(...)` literal.
    fn lch(value: &str) -> (f64, f64, f64) {
        let inner = value.trim_start_matches("oklch(").trim_end_matches(')');
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().unwrap());
        (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn light_anchors_hold_at_every_slider_position() {
        // THE headline rule: Light mode's paper is BRIGHT at every hue and
        // strength — the tint colours it, it never dims it — and the ink
        // stays mostly black with a whisper of the paper's hue.
        for (hue, strength) in [(34u16, 35u8), (104, 100), (200, 60), (350, 100)] {
            let t = strength as f64 / 100.0;
            let p = TextPalette::compute(&tinted(BaseMode::Light, hue, strength));
            let (pl, pc, ph) = lch(&p.paper);
            assert!((pl - 0.98).abs() < 1e-9, "paper L {pl} at {hue}/{strength}");
            assert!(pc > 0.0, "a tinted paper must carry colour");
            assert!((pc - 0.08 * t).abs() < 1e-3, "paper C {pc} at {strength}");
            let want_h = ui_hue_oklch(hue as f64);
            let d = (ph - want_h).abs().min(360.0 - (ph - want_h).abs());
            assert!(d < 1.0, "paper hue {ph} vs {want_h}");

            let (il, ic, ih) = lch(&p.ink);
            assert!((il - 0.15).abs() < 1e-9, "ink must stay mostly black, got L={il}");
            assert!((ic - 0.03 * t).abs() < 1e-3, "ink C {ic} — a whisper, not a paint job");
            assert_eq!(ih, ph, "the ink must take the paper's hue");
        }
    }

    #[test]
    fn dark_paper_is_a_darkish_grey_never_pitch_black() {
        let p = TextPalette::compute(&tinted(BaseMode::Dark, 104, 100));
        let (pl, pc, _) = lch(&p.paper);
        assert!((pl - 0.24).abs() < 1e-9, "paper L {pl}");
        assert!(pl > 0.2, "dark paper must stay a grey, not pure black");
        assert!((pc - 0.10).abs() < 1e-3, "paper C {pc}");
        let (il, ic, _) = lch(&p.ink);
        assert!((il - 0.92).abs() < 1e-9, "ink must stay mostly white, got L={il}");
        assert!((ic - 0.04).abs() < 1e-3, "ink C {ic}");
    }

    #[test]
    fn dim_is_medium_dark_paper_with_dark_ink() {
        let p = TextPalette::compute(&tinted(BaseMode::Dim, 0, 0));
        let (pl, pc, _) = lch(&p.paper);
        assert!((pl - 0.40).abs() < 1e-9, "paper L {pl}");
        assert_eq!(pc, 0.0, "untinted dim paper is neutral");
        let (il, _, _) = lch(&p.ink);
        assert!((il - 0.12).abs() < 1e-9, "dim ink stays dark/black, got L={il}");
        assert!(il < pl, "the ink must be darker than the dim paper");
        // Dark ink on the medium-dark paper must stay readable.
        assert!((pl + 0.05) / (il + 0.05) >= 2.5);
        // Medium-dark: between the dark and the light paper.
        assert!(pl > 0.24 && pl < 0.98, "grey {pl} must sit between the modes");
        assert_eq!(p.accent, base_tokens(BaseMode::Dim).accent);
    }

    #[test]
    fn untinted_palettes_are_achromatic_and_keep_the_base_accent() {
        // A strength-0 look must not colourise anything: chroma stays at
        // zero, and the accent keeps the base family (links do not turn
        // orange because the slider happens to sit at a warm default hue).
        let p = TextPalette::compute(&tinted(BaseMode::Light, 34, 0));
        assert_eq!(lch(&p.paper).1, 0.0);
        assert_eq!(lch(&p.ink).1, 0.0);
        assert_eq!(lch(&p.muted).1, 0.0);
        assert_eq!(p.accent, base_tokens(BaseMode::Light).accent);
        assert_eq!(p.accent_soft, base_tokens(BaseMode::Light).accent_soft);

        let d = TextPalette::compute(&tinted(BaseMode::Dark, 34, 0));
        assert_eq!(d.accent, base_tokens(BaseMode::Dark).accent);
    }

    #[test]
    fn the_ladder_holds_in_every_mode() {
        // Light and Dim follow the dark-ink direction: paper is the
        // brightest, borders sit between, ink is the darkest.
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let (paper, _, _) = lch(&p.paper);
        let (surface, _, _) = lch(&p.surface);
        let (line, _, _) = lch(&p.line);
        let (muted, _, _) = lch(&p.muted);
        let (ink, _, _) = lch(&p.ink);
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");
        assert!(line > muted + 0.01, "line {line} vs muted {muted}");
        assert!(muted > ink + 0.01, "muted {muted} vs ink {ink}");

        let m = TextPalette::compute(&tinted(BaseMode::Dim, 104, 100));
        let (mpaper, _, _) = lch(&m.paper);
        let (msurface, _, _) = lch(&m.surface);
        let (mline, _, _) = lch(&m.line);
        let (mmuted, _, _) = lch(&m.muted);
        let (mink, _, _) = lch(&m.ink);
        assert!(mpaper > msurface, "dim paper {mpaper} vs surface {msurface}");
        assert!(msurface > mline, "dim surface {msurface} vs line {mline}");
        assert!(mline > mmuted, "dim line {mline} vs muted {mmuted}");
        assert!(mmuted > mink, "dim muted {mmuted} vs ink {mink}");

        // Dark inverts: the paper is the darkest, the ink the brightest.
        let d = TextPalette::compute(&tinted(BaseMode::Dark, 104, 100));
        let (dpaper, _, _) = lch(&d.paper);
        let (dsurface, _, _) = lch(&d.surface);
        let (dline, _, _) = lch(&d.line);
        let (dmuted, _, _) = lch(&d.muted);
        let (dink, _, _) = lch(&d.ink);
        assert!(dpaper < dsurface, "dark paper {dpaper} vs surface {dsurface}");
        assert!(dsurface < dline, "dark surface {dsurface} vs line {dline}");
        assert!(dline < dmuted, "dark line {dline} vs muted {dmuted}");
        assert!(dmuted < dink, "dark muted {dmuted} vs ink {dink}");
    }

    #[test]
    fn full_strength_lands_paper_and_accent_on_the_slider_hue() {
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let want_h = ui_hue_oklch(104.0);
        let (_, _, ph) = lch(&p.paper);
        let d = (ph - want_h).abs().min(360.0 - (ph - want_h).abs());
        assert!(d < 1.0, "paper hue {ph} vs {want_h}");

        let (_, _, ah) = lch(&p.accent);
        let d = (ah - want_h).abs().min(360.0 - (ah - want_h).abs());
        assert!(d < 1.0, "accent hue {ah} vs {want_h}");
    }

    #[test]
    fn chroma_rises_with_strength_while_lightness_holds() {
        let weak = TextPalette::compute(&tinted(BaseMode::Light, 104, 20));
        let strong = TextPalette::compute(&tinted(BaseMode::Light, 104, 90));
        assert!(lch(&weak.paper).1 < lch(&strong.paper).1);
        assert_eq!(lch(&weak.paper).0, lch(&strong.paper).0);
    }

    #[test]
    fn the_palette_reports_its_own_paper_and_ink_lightness() {
        let p = TextPalette::compute(&tinted(BaseMode::Light, 0, 0));
        assert!((p.paper_l - 0.98).abs() < 1e-9);
        assert!((p.ink_l - 0.15).abs() < 1e-9);
        let d = TextPalette::compute(&tinted(BaseMode::Dark, 0, 0));
        assert!((d.paper_l - 0.24).abs() < 1e-9);
        assert!((d.ink_l - 0.92).abs() < 1e-9);
        let m = TextPalette::compute(&tinted(BaseMode::Dim, 0, 0));
        assert!((m.paper_l - 0.40).abs() < 1e-9);
        assert!((m.ink_l - 0.12).abs() < 1e-9);
    }
}
