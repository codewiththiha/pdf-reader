//! The `--tx-*` CSS custom properties for the text page palette — what
//! `styles/text.css` resolves the paper, ink, accents and chrome of a
//! text/Markdown page through.
//!
//! Written on `<html>` alongside the PDF pipeline's `--canvas-*` /
//! `--color-*` variables; the two namespaces are disjoint, so painting
//! both every appearance change costs nothing and lets a format swap
//! repaint with no extra wiring.

use crate::appearance_text::palette::TextPalette;

/// The palette as `--tx-*` pairs, in the order the stylesheet consumes them.
pub fn css_variables(palette: &TextPalette) -> Vec<(&'static str, String)> {
    vec![
        ("--tx-paper", palette.paper.clone()),
        ("--tx-ink", palette.ink.clone()),
        ("--tx-muted", palette.muted.clone()),
        ("--tx-surface", palette.surface.clone()),
        ("--tx-line", palette.line.clone()),
        ("--tx-accent", palette.accent.clone()),
        ("--tx-accent-soft", palette.accent_soft.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::Appearance;
    use crate::appearance_text::palette::TextPalette;

    #[test]
    fn the_variables_carry_the_palette_in_a_stable_order() {
        let p = TextPalette::compute(&Appearance::default());
        let vars = css_variables(&p);
        assert_eq!(vars.len(), 7);
        assert_eq!(vars[0], ("--tx-paper", p.paper.clone()));
        assert_eq!(vars[1], ("--tx-ink", p.ink.clone()));
        assert_eq!(vars[2], ("--tx-muted", p.muted.clone()));
        assert_eq!(vars[3], ("--tx-surface", p.surface.clone()));
        assert_eq!(vars[4], ("--tx-line", p.line.clone()));
        assert_eq!(vars[5], ("--tx-accent", p.accent.clone()));
        assert_eq!(vars[6], ("--tx-accent-soft", p.accent_soft.clone()));
    }
}
