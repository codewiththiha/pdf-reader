//! The text-specific appearance hooks. Text pages need no engine call —
//! their tokens repaint reactively from the CSS variables — so this side
//! of the pipeline is one pure step: recomputing the page palette.

use reader_core::appearance::Appearance;
use reader_core::appearance_text::palette::TextPalette;
use reader_core::appearance_text::tokens;

/// The `--tx-*` variables for an appearance: the palette derived and
/// flattened to the names `styles/text.css` consumes. Always painted,
/// alongside the PDF token set — the two namespaces are disjoint, so
/// whichever format is open finds its own tokens waiting.
pub fn token_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    tokens::css_variables(&TextPalette::compute(a))
}
