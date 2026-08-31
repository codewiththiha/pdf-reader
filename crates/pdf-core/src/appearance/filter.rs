//! The canvas filter pipeline, as numbers.
//!
//! WHY THIS EXISTS. `Appearance::canvas_filter()` used to be the only thing
//! that knew the pipeline, and it emitted a CSS string. The engine then
//! regex-parsed that string back into a 3×3 matrix and a per-channel offset
//! in order to bake it into the pixels. So the colour maths had two
//! implementations and a lossy text hop between them: Rust formatted
//! `sepia({x:.3})`, JavaScript had to know that exact spelling to read it
//! back. A format change on either side would silently change the colour, and
//! nothing in CI would notice — the engine's smoke test supplies its own
//! filter strings and never sees this crate's output.
//!
//! This module is the single definition. `canvas_filter()` and
//! `canvas_filter_matrix()` are both derived from one list of [`FilterOp`],
//! so the string and the numbers cannot drift; the engine receives the matrix
//! directly and only parses a string when it is handed one by something that
//! has no access to this crate.
//!
//! The per-function matrices follow the CSS Filter Effects spec, and the
//! engine's smoke harness re-implements the same maths independently as its
//! cross-check, so a divergence here fails CI rather than shipping.

use serde::Serialize;

/// One CSS filter function, the amount it is applied at, and the precision it
/// is written with.
///
/// This is the unit the theme composes its pipeline out of, and the only
/// place a filter function's CSS spelling and its matrix both live.
///
/// WHY THE PRECISION IS PART OF THE OP. The engine used to bake from the CSS
/// string, so what it saw was the ROUNDED amount, not the one the theme
/// computed: a tint at 60% strength asks for `saturate(1.3599999999999999)`
/// and the stylesheet says `saturate(1.360)`. Both paths have to agree on
/// which of those is the colour, so the rounding happens once, here, and the
/// matrix is built from the same rounded value the string carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterOp {
    pub name: &'static str,
    /// The function's argument, unitless. For `hue-rotate` this is degrees.
    pub amount: f64,
    /// Decimal places in the CSS spelling. `None` writes the value's shortest
    /// exact form, which is what the hand-written base pipelines always had
    /// (`brightness(0.8)`, `invert(0.92)`) and must keep having: the stylesheet
    /// output is not free to change shape.
    pub decimals: Option<u8>,
}

impl FilterOp {
    /// The amount exactly as the CSS variable carries it.
    ///
    /// Parsed back from the rendered digits rather than computed with a
    /// multiply-and-round: `(0.0165 * 1000).round() / 1000` is 0.016 while
    /// `format!("{:.3}", 0.0165)` prints 0.017, and the two have to agree or
    /// the baked page and the stylesheet's filter become two colours. Reading
    /// back the digits makes them identical by construction. The `unwrap_or`
    /// is unreachable — `to_css` only ever emits digits we just formatted.
    pub fn css_amount(&self) -> f64 {
        self.amount_text().parse().unwrap_or(self.amount)
    }

    /// The number as it appears between the parentheses.
    fn amount_text(&self) -> String {
        match self.decimals {
            None => format!("{}", self.amount),
            Some(d) => format!("{:.*}", d as usize, self.amount),
        }
    }

    /// The CSS spelling, as it appears inside `--canvas-filter`.
    ///
    /// `hue-rotate` carries a `deg` unit; the rest are bare numbers.
    pub fn to_css(&self) -> String {
        let unit = if self.name == "hue-rotate" { "deg" } else { "" };
        format!("{}({}{})", self.name, self.amount_text(), unit)
    }

    /// This op as a matrix. `None` for a function this pipeline does not
    /// model — the caller skips it, matching the engine's long-standing
    /// behaviour of ignoring tokens it does not recognise rather than
    /// dropping the whole pipeline.
    pub fn matrix(&self) -> Option<FilterMatrix> {
        FilterMatrix::css_token(self.name, self.css_amount())
    }
}

/// A composed colour transform: `out = m · in + o`, with `m` row-major and
/// `o` in 0..=1 channel units.
///
/// Serialises to `{ m: [9 numbers], o: [3 numbers] }`, which is the shape the
/// engine's `FilterMatrix` type already declares.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FilterMatrix {
    /// Row-major 3×3 matrix.
    pub m: [f64; 9],
    /// Per-channel offset, in 0..=1 units (NOT 0..=255).
    pub o: [f64; 3],
}

impl FilterMatrix {
    pub fn identity() -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            o: [0.0, 0.0, 0.0],
        }
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
    }

    /// `self` applied first, then `next` — the order CSS lists its filters in.
    ///
    /// `next · (self · x + self.o) + next.o`, so the accumulated offset is
    /// carried through the later matrix rather than added on afterwards.
    /// Getting this backwards is the classic filter-composition bug: it is
    /// invisible for pure scaling filters and obvious for `invert`.
    pub fn then(&self, next: &FilterMatrix) -> FilterMatrix {
        let mut m = [0.0f64; 9];
        let mut o = [0.0f64; 3];
        for r in 0..3 {
            for c in 0..3 {
                m[r * 3 + c] = next.m[r * 3] * self.m[c]
                    + next.m[r * 3 + 1] * self.m[3 + c]
                    + next.m[r * 3 + 2] * self.m[6 + c];
            }
            o[r] = next.m[r * 3] * self.o[0]
                + next.m[r * 3 + 1] * self.o[1]
                + next.m[r * 3 + 2] * self.o[2]
                + next.o[r];
        }
        FilterMatrix { m, o }
    }

    /// Fold a pipeline in application order. An unmodelled function is
    /// skipped, which is what the engine's string parser always did.
    pub fn compose(ops: &[FilterOp]) -> FilterMatrix {
        let mut acc = FilterMatrix::identity();
        for op in ops {
            if let Some(next) = op.matrix() {
                acc = acc.then(&next);
            }
        }
        acc
    }

    /// The matrix for one CSS filter function, or `None` if it is not one of
    /// the six this pipeline can emit.
    pub fn css_token(name: &str, amount: f64) -> Option<FilterMatrix> {
        let a = amount;
        match name {
            "invert" => {
                // `invert(a)`: `out = a + (1 - 2a) · in`. At a = 1 it is a
                // full negative; at 0.5 it is a no-op, which is what the
                // `1 - 2a` shape encodes.
                let k = 1.0 - 2.0 * a;
                Some(FilterMatrix {
                    m: [k, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, k],
                    o: [a, a, a],
                })
            }
            "brightness" => Some(FilterMatrix {
                m: [a, 0.0, 0.0, 0.0, a, 0.0, 0.0, 0.0, a],
                o: [0.0, 0.0, 0.0],
            }),
            "contrast" => {
                let off = 0.5 * (1.0 - a);
                Some(FilterMatrix {
                    m: [a, 0.0, 0.0, 0.0, a, 0.0, 0.0, 0.0, a],
                    o: [off, off, off],
                })
            }
            "saturate" => {
                // Interpolation toward the Rec. 709 luma.
                let t = 1.0 - a;
                let r = 0.213 * t;
                let g = 0.715 * t;
                let b = 0.072 * t;
                Some(FilterMatrix {
                    m: [r + a, g, b, r, g + a, b, r, g, b + a],
                    o: [0.0, 0.0, 0.0],
                })
            }
            "sepia" => {
                const S: [f64; 9] = [
                    0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131,
                ];
                let mut m = [0.0f64; 9];
                for i in 0..9 {
                    let ident = if i == 0 || i == 4 || i == 8 { 1.0 } else { 0.0 };
                    m[i] = (1.0 - a) * ident + a * S[i];
                }
                Some(FilterMatrix { m, o: [0.0, 0.0, 0.0] })
            }
            "hue-rotate" => {
                let th = a.to_radians();
                let c = th.cos();
                let s = th.sin();
                Some(FilterMatrix {
                    m: [
                        0.213 + 0.787 * c - 0.213 * s,
                        0.715 - 0.715 * c - 0.715 * s,
                        0.072 - 0.072 * c + 0.928 * s,
                        0.213 - 0.213 * c + 0.143 * s,
                        0.715 + 0.285 * c + 0.140 * s,
                        0.072 - 0.072 * c - 0.283 * s,
                        0.213 - 0.213 * c - 0.787 * s,
                        0.715 - 0.715 * c + 0.715 * s,
                        0.072 + 0.928 * c + 0.072 * s,
                    ],
                    o: [0.0, 0.0, 0.0],
                })
            }
            _ => None,
        }
    }

    /// Apply to one pixel, in 0..=1 channel units. The engine bakes through
    /// integer LUTs for speed; this is the readable definition the tests
    /// check the LUT maths against.
    pub fn apply(&self, r: f64, g: f64, b: f64) -> [f64; 3] {
        [
            self.m[0] * r + self.m[1] * g + self.m[2] * b + self.o[0],
            self.m[3] * r + self.m[4] * g + self.m[5] * b + self.o[1],
            self.m[6] * r + self.m[7] * g + self.m[8] * b + self.o[2],
        ]
    }
}

impl Default for FilterMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `hue-rotate(0)` and `saturate(1)` are both documented no-ops; so is
    /// `invert(0.5)`, which is the shape the invert matrix exists to encode.
    #[test]
    fn no_op_amounts_are_identity() {
        assert!(FilterMatrix::css_token("hue-rotate", 0.0).unwrap().is_identity());
        assert!(FilterMatrix::css_token("saturate", 1.0).unwrap().is_identity());
        assert!(FilterMatrix::css_token("brightness", 1.0).unwrap().is_identity());
        assert!(FilterMatrix::css_token("contrast", 1.0).unwrap().is_identity());
        // sepia(0) interpolates fully back to the identity matrix.
        assert!(FilterMatrix::css_token("sepia", 0.0).unwrap().is_identity());
        // invert(0.5) is the one no-op that carries a non-zero offset.
        let half = FilterMatrix::css_token("invert", 0.5).unwrap();
        assert!(half.m.iter().all(|v| v.abs() < 1e-12));
        assert_eq!(half.o, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn unknown_function_is_skipped_not_fatal() {
        assert!(FilterMatrix::css_token("drop-shadow", 4.0).is_none());
        let ops = [
            FilterOp { name: "brightness", amount: 0.5, decimals: None },
            FilterOp { name: "drop-shadow", amount: 4.0, decimals: None },
        ];
        assert_eq!(
            FilterMatrix::compose(&ops),
            FilterMatrix::css_token("brightness", 0.5).unwrap()
        );
    }

    /// `invert(1)` after `brightness(0.5)` must invert the DIMMED value, not
    /// dim the inverted one. This is the composition-order test: both orders
    /// agree on the matrix but differ on the offset, so a backwards fold
    /// shows up here and nowhere else.
    #[test]
    fn composition_carries_the_offset_through() {
        let dim = FilterMatrix::css_token("brightness", 0.5).unwrap();
        let flip = FilterMatrix::css_token("invert", 1.0).unwrap();
        let composed = dim.then(&flip);

        // White in: dimmed to 0.5, then inverted to 0.5.
        let [r, g, b] = composed.apply(1.0, 1.0, 1.0);
        assert!((r - 0.5).abs() < 1e-9, "{r}");
        assert!((g - 0.5).abs() < 1e-9);
        assert!((b - 0.5).abs() < 1e-9);

        // Black in: dimmed to 0, then inverted to 1.
        let [r, _, _] = composed.apply(0.0, 0.0, 0.0);
        assert!((r - 1.0).abs() < 1e-9, "{r}");

        // The reversed fold gives a different offset, so this asserts the
        // order is actually being tested.
        let backwards = flip.then(&dim);
        let [r, _, _] = backwards.apply(1.0, 1.0, 1.0);
        assert!((r - 0.5).abs() > 1e-9, "reversed fold should differ");
    }

    /// Composing with identity, on either side, changes nothing.
    #[test]
    fn identity_is_the_fold_unit() {
        let tinted = FilterMatrix::compose(&[
            FilterOp { name: "sepia", amount: 0.4, decimals: Some(3) },
            FilterOp { name: "hue-rotate", amount: 90.0, decimals: Some(1) },
        ]);
        let id = FilterMatrix::identity();
        assert_eq!(tinted.then(&id), tinted);
        assert_eq!(id.then(&tinted), tinted);
    }

    /// An empty pipeline is the identity, which is what makes Light with no
    /// tint skip the bake entirely.
    #[test]
    fn empty_pipeline_is_identity() {
        assert!(FilterMatrix::compose(&[]).is_identity());
    }

    /// The CSS spelling has to round-trip through a `name(amount)` parser,
    /// because that is what the engine's fallback path does with the string.
    #[test]
    fn css_spelling_stays_parseable() {
        for op in [
            FilterOp { name: "invert", amount: 0.92, decimals: None },
            FilterOp { name: "brightness", amount: 1.02, decimals: None },
            FilterOp { name: "sepia", amount: 0.55, decimals: Some(3) },
            FilterOp { name: "saturate", amount: 1.6, decimals: Some(3) },
            FilterOp { name: "hue-rotate", amount: 146.0, decimals: Some(1) },
        ] {
            let css = op.to_css();
            assert!(css.ends_with(')'), "{css}");
            let open = css.find('(').expect("opens");
            assert_eq!(&css[..open], op.name, "{css}");
            let arg = &css[open + 1..css.len() - 1];
            let trimmed = arg.strip_suffix("deg").unwrap_or(arg);
            let parsed: f64 = trimmed.parse().unwrap_or_else(|_| panic!("unparseable {css}"));
            assert_eq!(parsed, op.css_amount(), "{css} parsed to {parsed}");
        }
    }

    /// The base pipelines are hand-written CSS the stylesheet already carries,
    /// so their spelling is fixed: no trailing zeros, one decimal on
    /// `hue-rotate`. The tint chain is computed, so it keeps its three
    /// decimals. This is the shape `--canvas-filter` has always had.
    #[test]
    fn the_base_spelling_keeps_no_trailing_zeros() {
        assert_eq!(
            FilterOp { name: "brightness", amount: 0.8, decimals: None }.to_css(),
            "brightness(0.8)"
        );
        assert_eq!(
            FilterOp { name: "invert", amount: 0.92, decimals: None }.to_css(),
            "invert(0.92)"
        );
        assert_eq!(
            FilterOp { name: "contrast", amount: 0.9, decimals: None }.to_css(),
            "contrast(0.9)"
        );
        assert_eq!(
            FilterOp { name: "hue-rotate", amount: 180.0, decimals: None }.to_css(),
            "hue-rotate(180deg)"
        );
        assert_eq!(
            FilterOp { name: "sepia", amount: 0.33, decimals: Some(3) }.to_css(),
            "sepia(0.330)"
        );
        assert_eq!(
            FilterOp { name: "hue-rotate", amount: 76.0, decimals: Some(1) }.to_css(),
            "hue-rotate(76.0deg)"
        );
    }

    /// A computed amount that does not round-trip at the written precision has
    /// to give the SAME number to the stylesheet and to the matrix. This is
    /// the case that would otherwise make the baked page and the CSS-filtered
    /// page two slightly different colours.
    #[test]
    fn the_matrix_uses_the_rounded_amount_the_string_carries() {
        // 1.0 + 0.6 * 0.6 is 1.3599999999999999, and the stylesheet says 1.360.
        let op = FilterOp {
            name: "saturate",
            amount: 1.0 + 0.6 * 0.6,
            decimals: Some(3),
        };
        assert_eq!(op.to_css(), "saturate(1.360)");
        assert_eq!(op.css_amount(), 1.36);
        assert_eq!(
            op.matrix().unwrap(),
            FilterMatrix::css_token("saturate", 1.36).unwrap()
        );
    }

    /// `invert(0.92)` must leave white near-black and black near-white, with
    /// the 8% residue the amount implies.
    #[test]
    fn invert_behaves_like_invert() {
        let m = FilterMatrix::css_token("invert", 0.92).unwrap();
        let [r, _, _] = m.apply(1.0, 1.0, 1.0);
        assert!((r - 0.08).abs() < 1e-9, "white -> {r}");
        let [r, _, _] = m.apply(0.0, 0.0, 0.0);
        assert!((r - 0.92).abs() < 1e-9, "black -> {r}");
    }
}
