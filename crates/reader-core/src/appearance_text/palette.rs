//! The text-page palette: [`TextPalette::compute`] turns the shared
//! [`Appearance`] into the concrete colours a text/Markdown page paints.
//!
//! WHY ITS OWN MATH. A PDF page is an always-light raster, so base mode and
//! tint reach it through a CSS filter chain (see `appearance_pdf`) whose
//! numbers are chosen for bitmaps. A text page owns its paper and ink
//! outright, so it derives them directly — and deliberately NOT with the
//! PDF maths, because the two formats need different things:
//!
//!   * Light: the paper is a BRIGHT light colour wherever the tint slider
//!     sits, and the ink stays mostly black so it reads on it.
//!   * Dark: the paper is a DARK colour and the ink mostly white.
//!   * Dim: the paper is a darkish grey and the ink dark/black.
//!   * The ink always picks up a whisper of the paper's hue, so the pair
//!     reads as one look instead of black-on-coloured.
//!
//! THE RULES.
//!   1. Every mode anchors the paper's lightness AND the ink's. The tint
//!      moves hue and chroma only — a slider can never turn a light page
//!      murky, a dark page glaring, or a dim page bright.
//!   2. The ink's hue follows the paper's, at a small chroma fraction of
//!      it — the "suit the background" shift that keeps text readable.
//!   3. Dim is its own palette here, not a transform of Light: darkish
//!      grey paper, dark ink, and the dim accent family.

use crate::appearance::base::{BaseTokens, base_tokens};
use crate::appearance::shared::oklch::{hex_to_oklch, oklch_css};
use crate::appearance::shared::tint::ui_hue_oklch;
use crate::appearance::{Appearance, BaseMode};
use crate::appearance_text::contrast::ensure_contrast;

/// The ink/paper ratio a dim page must keep: enough for dark ink on the
/// grey paper, without pushing the ink into the chrome's darker dim ink.
pub const DIM_MIN_CONTRAST: f64 = 2.6;

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
    /// texture strokes key their dark/light family off (a dim TEXT page
    /// sits on light-grey paper while its chrome is dark).
    pub paper_l: f64,
    /// OKLCH lightness of `ink`.
    pub ink_l: f64,
}

/// The per-mode anchors: the paper and ink lightness the tint must never
/// move, and the chroma ceilings the tint may reach at full strength.
/// Paper ceilings are restrained (a pastel page, not a saturated one);
/// ink ceilings are a whisper, so text never goes coloured.
struct Anchor {
    paper_l: f64,
    paper_c: f64,
    ink_l: f64,
    ink_c: f64,
}

/// Bright paper, mostly-black ink.
const LIGHT: Anchor = Anchor { paper_l: 0.965, paper_c: 0.050, ink_l: 0.280, ink_c: 0.030 };
/// Dark paper, mostly-white ink.
const DARK: Anchor = Anchor { paper_l: 0.105, paper_c: 0.035, ink_l: 0.930, ink_c: 0.020 };
/// Darkish grey paper, dark ink (the ink lightness is derived below).
const DIM: Anchor = Anchor { paper_l: 0.720, paper_c: 0.030, ink_l: 0.238, ink_c: 0.025 };

impl TextPalette {
    /// Derive the palette for an appearance. Pure — no DOM, no engine.
    pub fn compute(a: &Appearance) -> TextPalette {
        match a.base {
            BaseMode::Light => Self::at(a, base_tokens(BaseMode::Light), LIGHT),
            BaseMode::Dark => Self::at(a, base_tokens(BaseMode::Dark), DARK),
            BaseMode::Dim => Self::dim(a),
        }
    }

    /// Light and Dark: the base palette untinted (byte-identical, so an
    /// untouched install looks exactly as before), the anchors + tint when
    /// the slider is up.
    fn at(a: &Appearance, base: BaseTokens, anchor: Anchor) -> TextPalette {
        if a.has_tint() {
            Self::tinted(a, &base, anchor)
        } else {
            TextPalette {
                paper: base.paper.to_string(),
                ink: base.ink.to_string(),
                muted: base.muted.to_string(),
                surface: base.surface.to_string(),
                line: base.line.to_string(),
                accent: base.accent.to_string(),
                accent_soft: base.accent_soft.to_string(),
                paper_l: hex_to_oklch(base.paper).map(|(l, _, _)| l).unwrap_or(anchor.paper_l),
                ink_l: hex_to_oklch(base.ink).map(|(l, _, _)| l).unwrap_or(anchor.ink_l),
            }
        }
    }

    /// Dim: its own palette. Untinted it is the grey page — mid-light
    /// paper, dark ink, the dim accent family (the bright Light accent
    /// would glare on grey); tinted it anchors the same grey lightness and
    /// lets the slider colour it.
    fn dim(a: &Appearance) -> TextPalette {
        let light = base_tokens(BaseMode::Light);
        let light_ink_l = hex_to_oklch(light.ink).map(|(l, _, _)| l).unwrap_or(0.28);
        // Dark ink on the grey paper, held to the dim floor.
        let ink_l = ensure_contrast(light_ink_l, DIM.paper_l, DIM_MIN_CONTRAST);
        let anchor = Anchor { ink_l, ..DIM };
        if a.has_tint() {
            return Self::tinted(a, &light, anchor);
        }
        let dim_base = base_tokens(BaseMode::Dim);
        let paper_l = anchor.paper_l;
        TextPalette {
            paper: oklch_css(paper_l, 0.0, 0.0),
            ink: oklch_css(ink_l, 0.0, 0.0),
            muted: oklch_css(lerp(paper_l, ink_l, 0.45), 0.0, 0.0),
            surface: oklch_css(lerp(paper_l, ink_l, 0.06), 0.0, 0.0),
            line: oklch_css(lerp(paper_l, ink_l, 0.14), 0.0, 0.0),
            accent: dim_base.accent.to_string(),
            accent_soft: dim_base.accent_soft.to_string(),
            paper_l,
            ink_l,
        }
    }

    /// The tinted palette: every token anchored to the mode's lightness
    /// ladder, hue moved onto the slider's colour, chroma scaled with
    /// strength. The ink follows the paper's hue so the two read as one
    /// look, at a fraction of the paper's chroma so text stays text.
    fn tinted(a: &Appearance, base: &BaseTokens, anchor: Anchor) -> TextPalette {
        let t = a.tint_strength as f64 / 100.0;
        let target_h = ui_hue_oklch(a.tint_hue as f64);
        let paper_l = anchor.paper_l;
        let ink_l = anchor.ink_l;

        let paper = oklch_css(paper_l, anchor.paper_c * t, target_h);
        let ink = oklch_css(ink_l, anchor.ink_c * t, target_h);
        let muted = oklch_css(lerp(paper_l, ink_l, 0.45), anchor.ink_c * 0.6 * t, target_h);
        let surface = oklch_css(lerp(paper_l, ink_l, 0.06), anchor.paper_c * 0.6 * t, target_h);
        let line = oklch_css(lerp(paper_l, ink_l, 0.14), anchor.paper_c * 0.4 * t, target_h);

        // The accent keeps its OWN lightness (the same invariant the PDF
        // tint holds) and walks its hue toward the paper's, so links suit
        // the tinted page rather than clashing with it.
        let (al, ac0, ah0) = hex_to_oklch(base.accent).unwrap_or((0.5, 0.1, 0.0));
        let mut delta = target_h - ah0;
        while delta > 180.0 {
            delta -= 360.0;
        }
        while delta < -180.0 {
            delta += 360.0;
        }
        let accent_h = (ah0 + delta * t).rem_euclid(360.0);
        let accent_c = ac0 + (0.130 - ac0).max(0.0) * t;
        let accent = oklch_css(al, accent_c, accent_h);
        let accent_soft = oklch_css(lerp(paper_l, al, 0.25), accent_c * 0.35, accent_h);

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
    use crate::appearance::shared::oklch::hex_to_oklch;
    use crate::appearance_text::contrast::contrast_ratio;

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
    fn light_tint_keeps_the_paper_bright_no_matter_where_the_slider_sits() {
        // THE headline rule: Light mode's paper is a BRIGHT light colour at
        // every hue and strength — the tint colours it, it never dims it.
        for (hue, strength) in [(34u16, 35u8), (104, 100), (200, 60), (350, 100)] {
            let p = TextPalette::compute(&tinted(BaseMode::Light, hue, strength));
            let (pl, pc, ph) = lch(&p.paper);
            assert!((pl - 0.965).abs() < 0.001, "paper L {pl} at {hue}/{strength}");
            assert!(pc > 0.0, "a tinted paper must carry colour");
            let want_h = ui_hue_oklch(hue as f64);
            let d = (ph - want_h).abs().min(360.0 - (ph - want_h).abs());
            assert!(d < 1.0, "paper hue {ph} vs {want_h}");
        }
    }

    #[test]
    fn light_tint_keeps_the_ink_mostly_black_and_whispers_the_paper_hue() {
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let (il, ic, ih) = lch(&p.ink);
        assert!((il - 0.28).abs() < 0.001, "ink must stay mostly black, got L={il}");
        assert!(ic > 0.0 && ic < 0.035, "ink chroma {ic} — a whisper, not a paint job");
        let (_, _, ph) = lch(&p.paper);
        assert_eq!(ih, ph, "the ink must take the paper's hue");
    }

    #[test]
    fn dark_tint_keeps_the_paper_darkish_and_the_ink_mostly_white() {
        let p = TextPalette::compute(&tinted(BaseMode::Dark, 104, 80));
        let (pl, pc, _) = lch(&p.paper);
        assert!((pl - 0.105).abs() < 0.001, "paper L {pl}");
        assert!(pc < 0.04, "dark paper chroma {pc} must stay darkish");
        let (il, _, _) = lch(&p.ink);
        assert!((il - 0.93).abs() < 0.001, "ink must stay mostly white, got L={il}");
    }

    #[test]
    fn chroma_rises_with_strength_while_lightness_holds() {
        let weak = TextPalette::compute(&tinted(BaseMode::Light, 104, 20));
        let strong = TextPalette::compute(&tinted(BaseMode::Light, 104, 90));
        assert!(lch(&weak.paper).1 < lch(&strong.paper).1);
        assert_eq!(lch(&weak.paper).0, lch(&strong.paper).0);
    }

    #[test]
    fn dim_is_a_darkish_grey_page_with_dark_ink() {
        let p = TextPalette::compute(&tinted(BaseMode::Dim, 0, 0));
        let (pl, pc, _) = lch(&p.paper);
        assert!((pl - 0.72).abs() < 0.001, "paper L {pl}");
        assert_eq!(pc, 0.0, "untinted dim paper is neutral grey");
        let (il, _, _) = lch(&p.ink);
        assert!(il < 0.3, "dim ink stays dark/black, got L={il}");
        assert!(contrast_ratio(il, pl) >= DIM_MIN_CONTRAST);
        // The grey page sits between the light and the dark papers...
        let lp = hex_to_oklch(base_tokens(BaseMode::Light).paper).unwrap().0;
        let dp = hex_to_oklch(base_tokens(BaseMode::Dark).paper).unwrap().0;
        assert!(pl < lp && pl > dp, "grey {pl} must sit between {lp} and {dp}");
        // ...and its accent stays the dim family.
        assert_eq!(p.accent, base_tokens(BaseMode::Dim).accent);
    }

    #[test]
    fn dim_tint_keeps_the_grey_paper_grey_and_the_ink_dark() {
        let p = TextPalette::compute(&tinted(BaseMode::Dim, 104, 50));
        let (pl, pc, ph) = lch(&p.paper);
        assert!((pl - 0.72).abs() < 0.001, "paper L {pl}");
        assert!(pc > 0.0, "the slider colour must land on the dim paper too");
        let want_h = ui_hue_oklch(104.0);
        let d = (ph - want_h).abs().min(360.0 - (ph - want_h).abs());
        assert!(d < 1.0);
        let (il, _, ih) = lch(&p.ink);
        assert!(il < 0.3, "dim ink stays dark under a tint too");
        assert_eq!(ih, ph, "the ink must follow the tinted paper");
    }

    #[test]
    fn the_lightness_ladder_survives_the_tint() {
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let paper = lch(&p.paper).0;
        let surface = lch(&p.surface).0;
        let line = lch(&p.line).0;
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");

        let d = TextPalette::compute(&tinted(BaseMode::Dark, 104, 100));
        let dpaper = lch(&d.paper).0;
        let dsurface = lch(&d.surface).0;
        assert!(dpaper < dsurface, "dark paper must stay the darkest");
    }

    #[test]
    fn the_accent_walks_toward_the_paper_hue_without_leaving_its_lightness() {
        let base_accent = base_tokens(BaseMode::Light).accent;
        let base_l = hex_to_oklch(base_accent).unwrap().0;
        let p = TextPalette::compute(&tinted(BaseMode::Light, 104, 100));
        let (al, _, ah) = lch(&p.accent);
        assert!((al - base_l).abs() < 0.01, "accent L {al} vs {base_l}");
        let want_h = ui_hue_oklch(104.0);
        let d = (ah - want_h).abs().min(360.0 - (ah - want_h).abs());
        assert!(d < 1.0, "a full-strength tint must land the accent on the slider hue");
    }

    #[test]
    fn the_palette_reports_its_own_paper_and_ink_lightness() {
        let p = TextPalette::compute(&Appearance::default());
        assert!((p.paper_l - 1.0).abs() < 0.002);
        assert!(p.ink_l < 0.3);
        let d = TextPalette::compute(&tinted(BaseMode::Dark, 0, 0));
        assert!(d.paper_l < 0.2);
        assert!(d.ink_l > 0.9);
    }
}
