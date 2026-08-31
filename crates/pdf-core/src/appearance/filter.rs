//! The canvas filter as structured maths: per-token matrices, composition,
//! and the raster pixel loop.
//!
//! The CSS pipeline (`invert() hue-rotate() saturate() …`) painted onto
//! `--canvas-filter` is one linear colour transform: a 3×3 channel matrix
//! plus per-channel offsets. Historically only the CSS *string* existed on
//! the Rust side and the JS engine re-derived the matrix by tokenizing that
//! string — a lossy re-implementation of maths Rust had already computed.
//! This module is the single source of both views: the same token list feeds
//! [`Appearance::canvas_filter`] (CSS text) and
//! [`Appearance::canvas_filter_matrix`] (composed numbers), and
//! [`bake_pixels`] runs the per-pixel application that the engine's WASM
//! side registers as its hot-path baker.
//!
//! The arithmetic deliberately mirrors the engine's JS implementation
//! (public/engine/theme/bake.ts) value-for-value — including
//! `Math.round`-style half-away rounding (`floor(x + 0.5)`) and the 16.16
//! fixed-point LUT scheme — so a bake from the structured matrix and the old
//! string-reparsed bake agree to the pixel.

use serde::{Deserialize, Serialize};

/// A colour transform: `out_channel = Σ m[row·3 + k] · in_k + o[row]`, with
/// channels in 0..=255 and offsets in 0..=1 (× 255 at application time).
///
/// Serialized across the engine bridge as `{ m: [9 numbers], o: [3 numbers] }`
/// — the wire twin of the JS `FilterMatrix` type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterMatrix {
    /// Row-major 3×3 channel matrix.
    pub m: [f64; 9],
    /// Per-channel offsets, in 0..=1 units.
    pub o: [f64; 3],
}

impl FilterMatrix {
    pub const fn identity() -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            o: [0.0, 0.0, 0.0],
        }
    }

    /// Exact identity test, mirroring the JS baker's early-out: an identity
    /// filter must not force a compositing pass over the pixels at all.
    pub fn is_identity(&self) -> bool {
        self.m == [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] && self.o == [0.0, 0.0, 0.0]
    }

    /// Build from loose slices (the bridge hands plain arrays over). `None`
    /// when the shapes are not 9 + 3.
    pub fn from_slice(m: &[f64], o: &[f64]) -> Option<Self> {
        if m.len() != 9 || o.len() != 3 {
            return None;
        }
        let mut mm = [0.0; 9];
        mm.copy_from_slice(m);
        let mut oo = [0.0; 3];
        oo.copy_from_slice(o);
        Some(Self { m: mm, o: oo })
    }
}

/// One CSS filter token, kept as (kind, numeric argument, verbatim CSS) so
/// the string painted to the stylesheet and the matrix handed to the raster
/// baker are built from one list and can never drift apart.
pub(crate) struct FilterOp {
    pub(crate) kind: FilterKind,
    pub(crate) arg: f64,
    /// The exact token text `canvas_filter()` joins into the CSS value —
    /// precision is per-site and load-bearing (tests assert exact strings).
    pub(crate) css: String,
}

impl FilterOp {
    pub(crate) fn new(kind: FilterKind, arg: f64, css: String) -> Self {
        Self { kind, arg, css }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FilterKind {
    Invert,
    Brightness,
    Contrast,
    Saturate,
    Sepia,
    HueRotate,
}

impl FilterKind {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "invert" => Self::Invert,
            "brightness" => Self::Brightness,
            "contrast" => Self::Contrast,
            "saturate" => Self::Saturate,
            "sepia" => Self::Sepia,
            "hue-rotate" => Self::HueRotate,
            _ => return None,
        })
    }
}

/// `op` applied after `acc` — the exact composition the JS `composeFilter`
/// performs per token (row of `op` times columns of `acc`, offsets folded).
fn apply_after(op: &FilterMatrix, acc: &FilterMatrix) -> FilterMatrix {
    let mut out = FilterMatrix::identity();
    for r in 0..3 {
        for c in 0..3 {
            out.m[r * 3 + c] = op.m[r * 3] * acc.m[c]
                + op.m[r * 3 + 1] * acc.m[3 + c]
                + op.m[r * 3 + 2] * acc.m[6 + c];
        }
        out.o[r] = op.m[r * 3] * acc.o[0]
            + op.m[r * 3 + 1] * acc.o[1]
            + op.m[r * 3 + 2] * acc.o[2]
            + op.o[r];
    }
    out
}

/// The per-token matrices, value-for-value the ones the engine's JS
/// `filterTokenToMatrix` builds (CSS Filter Effects sRGB forms).
pub(crate) fn token_matrix(kind: FilterKind, arg: f64) -> FilterMatrix {
    let diag = |k: f64| FilterMatrix {
        m: [k, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, k],
        o: [0.0, 0.0, 0.0],
    };
    match kind {
        FilterKind::Invert => {
            let k = 1.0 - 2.0 * arg;
            FilterMatrix {
                m: [k, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, k],
                o: [arg, arg, arg],
            }
        }
        FilterKind::Brightness => diag(arg),
        FilterKind::Contrast => {
            let off = 0.5 * (1.0 - arg);
            FilterMatrix {
                m: diag(arg).m,
                o: [off, off, off],
            }
        }
        FilterKind::Saturate => {
            let t = 1.0 - arg;
            let a = 0.213 * t;
            let b = 0.715 * t;
            let c = 0.072 * t;
            FilterMatrix {
                m: [a + arg, b, c, a, b + arg, c, a, b, c + arg],
                o: [0.0, 0.0, 0.0],
            }
        }
        FilterKind::Sepia => {
            const S: [f64; 9] = [
                0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131,
            ];
            let mut m = [0.0; 9];
            for (i, v) in m.iter_mut().enumerate() {
                let ident = if i == 0 || i == 4 || i == 8 { 1.0 } else { 0.0 };
                *v = (1.0 - arg) * ident + arg * S[i];
            }
            FilterMatrix {
                m,
                o: [0.0, 0.0, 0.0],
            }
        }
        FilterKind::HueRotate => {
            let th = arg * std::f64::consts::PI / 180.0;
            let c = th.cos();
            let s = th.sin();
            FilterMatrix {
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
            }
        }
    }
}

/// Compose a token list into one matrix (first token innermost).
pub(crate) fn compose_filter_ops(ops: &[FilterOp]) -> FilterMatrix {
    let mut acc = FilterMatrix::identity();
    for op in ops {
        acc = apply_after(&token_matrix(op.kind, op.arg), &acc);
    }
    acc
}

/// Compose a CSS filter string into one matrix — the Rust reference of the
/// engine's string fallback. Unknown tokens are skipped, exactly like the JS
/// parser; `hue-rotate(90deg)`'s unit suffix is accepted (that is the only
/// unit the app's own strings ever carry).
pub fn compose_filter_string(filter: &str) -> FilterMatrix {
    let mut acc = FilterMatrix::identity();
    for tok in filter.split_whitespace() {
        let Some((name, arg)) = parse_token(tok) else {
            continue;
        };
        let Some(kind) = FilterKind::from_name(name) else {
            continue;
        };
        acc = apply_after(&token_matrix(kind, arg), &acc);
    }
    acc
}

fn parse_token(tok: &str) -> Option<(&str, f64)> {
    let tok = tok.trim();
    let open = tok.find('(')?;
    let close = tok.rfind(')')?;
    if close <= open {
        return None;
    }
    let name = &tok[..open];
    let raw = tok[open + 1..close].trim();
    let raw = raw.strip_suffix("deg").unwrap_or(raw);
    let arg: f64 = raw.parse().ok()?;
    Some((name, arg))
}

/// `Math.round` semantics: halves go toward +∞ (`floor(x + 0.5)`), which
/// differs from Rust's `f64::round` (halves away from zero) on negative
/// halves — and the invert chain's LUT entries are frequently negative.
fn js_round(x: f64) -> i32 {
    (x + 0.5).floor() as i32
}

/// Apply the transform to an RGBA buffer in place, alpha untouched.
///
/// The scheme is the JS baker's: per-coefficient 256-entry LUTs in 16.16
/// fixed point (each entry rounded like `Math.round`), three LUT reads plus
/// an offset per channel, then an arithmetic `>> 16` — i.e. truncation
/// toward −∞, matching the JS `>>` — and finally the clamping a
/// `Uint8ClampedArray` assignment performs implicitly.
pub fn bake_pixels(pixels: &mut [u8], filter: &FilterMatrix) {
    if filter.is_identity() {
        return;
    }
    const SCALE: f64 = 65536.0;

    let mut luts = [[0i32; 256]; 9];
    for (i, lut) in luts.iter_mut().enumerate() {
        let coef = filter.m[i];
        for (v, slot) in lut.iter_mut().enumerate() {
            *slot = js_round(coef * v as f64 * SCALE);
        }
    }
    let o = filter.o.map(|x| js_round(x * 255.0 * SCALE));

    let [l0, l1, l2, l3, l4, l5, l6, l7, l8] = luts;
    let [o0, o1, o2] = o;
    // Full RGBA quads; a ragged tail (never produced by ImageData) is left
    // untouched, exactly like the JS loop's `i += 4` stride.
    let (quads, _tail) = pixels.as_chunks_mut::<4>();
    for px in quads {
        let r = px[0] as usize;
        let g = px[1] as usize;
        let b = px[2] as usize;
        px[0] = ((l0[r] + l1[g] + l2[b] + o0) >> 16).clamp(0, 255) as u8;
        px[1] = ((l3[r] + l4[g] + l5[b] + o1) >> 16).clamp(0, 255) as u8;
        px[2] = ((l6[r] + l7[g] + l8[b] + o2) >> 16).clamp(0, 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{Appearance, BaseMode};

    fn approx(a: &FilterMatrix, b: &FilterMatrix, tol: f64) {
        for i in 0..9 {
            assert!(
                (a.m[i] - b.m[i]).abs() <= tol,
                "m[{i}]: {} vs {}",
                a.m[i],
                b.m[i]
            );
        }
        for i in 0..3 {
            assert!(
                (a.o[i] - b.o[i]).abs() <= tol,
                "o[{i}]: {} vs {}",
                a.o[i],
                b.o[i]
            );
        }
    }

    // --- token matrices -----------------------------------------------------

    #[test]
    fn js_round_halves_toward_positive_infinity() {
        // Math.round(-0.5) is -0 and Math.round(-1.5) is -1; Rust's round()
        // would give -1 and -2. The LUT path must follow JS.
        assert_eq!(js_round(0.5), 1);
        assert_eq!(js_round(-0.5), 0);
        assert_eq!(js_round(1.5), 2);
        assert_eq!(js_round(-1.5), -1);
        assert_eq!(js_round(-2.5), -2);
    }

    #[test]
    fn token_matrices_match_the_reference_formulas() {
        // invert(0.92): k = 1 - 2·0.92 = -0.84, offsets 0.92. (The arithmetic
        // lands one ulp off the decimal literal — exactly what the JS baker's
        // identical expression produces, so parity is the point.)
        let inv = token_matrix(FilterKind::Invert, 0.92);
        assert!((inv.m[0] + 0.84).abs() < 1e-12, "{}", inv.m[0]);
        assert!((inv.m[4] + 0.84).abs() < 1e-12, "{}", inv.m[4]);
        assert!((inv.m[8] + 0.84).abs() < 1e-12, "{}", inv.m[8]);
        assert!((inv.o[0] - 0.92).abs() < 1e-12);
        assert!((inv.o[1] - 0.92).abs() < 1e-12);
        assert!((inv.o[2] - 0.92).abs() < 1e-12);

        // saturate(0.75): t = 0.25 → luminance row weights + arg on the diagonal.
        let sat = token_matrix(FilterKind::Saturate, 0.75);
        assert!((sat.m[0] - 0.80325).abs() < 1e-12, "{}", sat.m[0]);
        assert!((sat.m[1] - 0.17875).abs() < 1e-12);
        assert!((sat.m[4] - 0.92875).abs() < 1e-12);
        assert_eq!(sat.o, [0.0, 0.0, 0.0]);

        // contrast(0.9): diag 0.9 with offset 0.05 per channel (0.5·(1-0.9)
        // is one ulp under 0.05 — same as JS).
        let con = token_matrix(FilterKind::Contrast, 0.9);
        assert_eq!(con.m[0], 0.9);
        for v in con.o {
            assert!((v - 0.05).abs() < 1e-12, "offset {v}");
        }

        // sepia(1) is the pure sepia matrix.
        let sep = token_matrix(FilterKind::Sepia, 1.0);
        assert!((sep.m[0] - 0.393).abs() < 1e-12);
        assert!((sep.m[4] - 0.686).abs() < 1e-12);

        // saturate(0) collapses to the luminance matrix.
        let gray = token_matrix(FilterKind::Saturate, 0.0);
        assert!((gray.m[0] - 0.213).abs() < 1e-12);
        assert!((gray.m[1] - 0.715).abs() < 1e-12);
        assert!((gray.m[2] - 0.072).abs() < 1e-12);
        assert!((gray.m[8] - 0.072).abs() < 1e-12);
    }

    // --- composition --------------------------------------------------------

    #[test]
    fn dim_chain_composes_to_the_hand_computed_matrix() {
        // brightness(0.8) then saturate(0.75) then contrast(0.9):
        // brightness and contrast are diagonal, so each scales the saturate
        // matrix elementwise (0.8 then 0.9); contrast also adds 0.05 to
        // every offset.
        let ops = [
            FilterOp::new(FilterKind::Brightness, 0.8, "brightness(0.8)".into()),
            FilterOp::new(FilterKind::Saturate, 0.75, "saturate(0.75)".into()),
            FilterOp::new(FilterKind::Contrast, 0.9, "contrast(0.9)".into()),
        ];
        let got = compose_filter_ops(&ops);
        let sat = token_matrix(FilterKind::Saturate, 0.75);
        for r in 0..3 {
            for c in 0..3 {
                let want = sat.m[r * 3 + c] * 0.8 * 0.9;
                assert!(
                    (got.m[r * 3 + c] - want).abs() < 1e-12,
                    "m[{}]: {} vs {want}",
                    r * 3 + c,
                    got.m[r * 3 + c]
                );
            }
        }
        // Contrast's offset is 0.5·(1-0.9), one ulp under the decimal 0.05 —
        // the same value the JS expression produces.
        for v in got.o {
            assert!((v - 0.05).abs() < 1e-12, "offset {v}");
        }
    }

    #[test]
    fn string_and_ops_composition_agree_for_the_real_chains() {
        // Base chains serialize their arguments exactly, so parity is tight.
        let dark = Appearance {
            base: BaseMode::Dark,
            ..Default::default()
        };
        approx(
            &compose_filter_string(&dark.canvas_filter()),
            &dark.canvas_filter_matrix(),
            1e-12,
        );

        let dim = Appearance {
            base: BaseMode::Dim,
            ..Default::default()
        };
        approx(
            &compose_filter_string(&dim.canvas_filter()),
            &dim.canvas_filter_matrix(),
            1e-12,
        );

        // Tint chain: every argument round-trips through {:.3}/{:.1} exactly
        // at strength 50 / hue 124 (0.275, 1.300, 90.0), so parity is still
        // tight; a strength whose sepia does NOT round-trip (33% → 0.1815 →
        // "0.182") stays within the CSS rounding step.
        let tinted = Appearance {
            base: BaseMode::Light,
            tint_hue: 124,
            tint_strength: 50,
            ..Default::default()
        };
        approx(
            &compose_filter_string(&tinted.canvas_filter()),
            &tinted.canvas_filter_matrix(),
            1e-12,
        );

        let unruly = Appearance {
            base: BaseMode::Light,
            tint_hue: 77,
            tint_strength: 33,
            ..Default::default()
        };
        approx(
            &compose_filter_string(&unruly.canvas_filter()),
            &unruly.canvas_filter_matrix(),
            1e-2,
        );
    }

    #[test]
    fn compose_filter_string_skips_what_it_does_not_recognise() {
        // "none" and unknown tokens are skipped (the JS fallback behaviour),
        // and a hue-rotate argument carries its deg unit.
        let none = compose_filter_string("none");
        assert!(none.is_identity());
        let junk = compose_filter_string("blur(4px) invert(1)");
        assert_eq!(junk.m[0], -1.0);
        let deg = compose_filter_string("hue-rotate(90deg)");
        let bare = compose_filter_string("hue-rotate(90)");
        approx(&deg, &bare, 1e-12);
    }

    #[test]
    fn from_slice_validates_the_shapes() {
        assert!(FilterMatrix::from_slice(&[1.0; 8], &[0.0; 3]).is_none());
        assert!(FilterMatrix::from_slice(&[1.0; 9], &[0.0; 2]).is_none());
        let m = FilterMatrix::from_slice(&[2.0; 9], &[0.25; 3]).unwrap();
        assert_eq!(m.m[7], 2.0);
        assert_eq!(m.o[1], 0.25);
    }

    // --- bake_pixels --------------------------------------------------------

    #[test]
    fn identity_matrix_leaves_the_buffer_untouched() {
        let mut px = [1u8, 2, 3, 4, 200, 100, 50, 255];
        let before = px;
        bake_pixels(&mut px, &FilterMatrix::identity());
        assert_eq!(px, before);
    }

    #[test]
    fn invert_bakes_to_the_complement() {
        let mut px = [255u8, 0, 128, 255, 7, 250, 99, 255];
        bake_pixels(
            &mut px,
            &FilterMatrix {
                m: [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
                o: [1.0, 1.0, 1.0],
            },
        );
        assert_eq!(px, [0, 255, 127, 255, 248, 5, 156, 255]);
    }

    #[test]
    fn output_clamps_like_a_uint8clamped_array_assignment() {
        // brightness(2): 200 → 400 → 255; a negative-only matrix clamps to 0.
        let mut px = [200u8, 30, 255, 255];
        bake_pixels(&mut px, &token_matrix(FilterKind::Brightness, 2.0));
        assert_eq!(&px[..3], &[255, 60, 255]);

        let mut neg = [10u8, 200, 40, 255];
        bake_pixels(
            &mut neg,
            &FilterMatrix {
                m: [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
                o: [0.0, 0.0, 0.0],
            },
        );
        assert_eq!(&neg[..3], &[0, 0, 0]);
    }

    #[test]
    fn luts_stay_within_one_step_of_the_float_math() {
        // The LUT path truncates (>> 16) after rounding each coefficient
        // term; a direct float evaluation of the same transform must agree
        // to within one quantum per channel on every pixel.
        let dark = Appearance {
            base: BaseMode::Dark,
            tint_hue: 200,
            tint_strength: 60,
            ..Default::default()
        };
        let filter = dark.canvas_filter_matrix();
        let mut px = [17u8, 240, 99, 255, 0, 1, 254, 255, 128, 64, 32, 255, 200, 200, 200, 255];
        let baked = px;
        bake_pixels(&mut px, &filter);
        for quad in 0..px.len() / 4 {
            let base = quad * 4;
            let (r, g, b) = (baked[base] as f64, baked[base + 1] as f64, baked[base + 2] as f64);
            for ch in 0..3 {
                let want = (filter.m[ch * 3] * r
                    + filter.m[ch * 3 + 1] * g
                    + filter.m[ch * 3 + 2] * b
                    + filter.o[ch] * 255.0)
                    .clamp(0.0, 255.0);
                assert!(
                    (px[base + ch] as f64 - want).abs() <= 1.0,
                    "quad {quad} ch {ch}: {} vs {want}",
                    px[base + ch]
                );
            }
        }
    }

    #[test]
    fn a_trailing_partial_pixel_is_left_alone() {
        // ImageData lengths are always multiples of 4, but the baker must
        // stay safe on a ragged buffer: chunks_exact_mut keeps the tail.
        let mut px = [10u8, 20, 30, 40, 99, 98];
        let filter = FilterMatrix {
            m: [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
            o: [1.0, 1.0, 1.0],
        };
        bake_pixels(&mut px, &filter);
        assert_eq!(&px[4..], &[99, 98]);
        assert_eq!(&px[..4], &[245, 235, 225, 40]);
    }
}
