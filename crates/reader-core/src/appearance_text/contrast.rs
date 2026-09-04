//! The ink/paper contrast guard for the text pipeline.
//!
//! The tint never moves lightness, so Light/Dark tokens keep the base
//! palettes' contrast by construction. Only dim moves it — it darkens the
//! paper the way the raster pipeline dims the page — so the dim ink is
//! re-derived here: pushed away from the paper until the ratio passes
//! [`MIN_TEXT_CONTRAST`].
//!
//! The ratio uses the OKLCH lightness the rest of the pipeline works in
//! rather than WCAG's sRGB luminance: an approximation that keeps this
//! crate dependency-free and reads the same numbers the palette code
//! already computes. (The Light palette's own ink/paper ratio is ≈3.2 in
//! this space — see the test below.)

/// The minimum ink/paper ratio the dim re-derivation targets: the Light
/// palette's own ink/paper ratio in the OKLCH-lightness approximation.
/// The dimmed paper is dark, so the re-derivation converges on the 0.95
/// ink ceiling before it can reach this; the constant binds the
/// (hypothetically) still-light branch and any future dim that stays
/// light.
pub const MIN_TEXT_CONTRAST: f64 = 3.2;

/// Approximate contrast ratio from two OKLCH lightnesses.
pub fn contrast_ratio(l1: f64, l2: f64) -> f64 {
    let lighter = l1.max(l2);
    let darker = l1.min(l2);
    (lighter + 0.05) / (darker + 0.05)
}

/// Push `ink_l` away from `paper_l` until the ratio reaches `min_ratio`.
/// Light paper darkens the ink; dark paper lightens it. Values that
/// already pass come back untouched.
pub fn ensure_contrast(ink_l: f64, paper_l: f64, min_ratio: f64) -> f64 {
    if contrast_ratio(ink_l, paper_l) >= min_ratio {
        return ink_l;
    }
    let mut l = ink_l;
    if paper_l > 0.5 {
        while contrast_ratio(l, paper_l) < min_ratio && l > 0.0 {
            l -= 0.02;
        }
    } else {
        while contrast_ratio(l, paper_l) < min_ratio && l < 1.0 {
            l += 0.02;
        }
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_lightnesses_have_a_ratio_of_one() {
        assert!((contrast_ratio(0.5, 0.5) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_light_palette_sets_the_floor() {
        // #1f2937 on #ffffff: the reader's default reading pair. The guard's
        // target is this ratio, so a dim page never reads worse than Light.
        let ink = crate::appearance::shared::oklch::hex_to_oklch("#1f2937").unwrap().0;
        let paper = 1.0;
        let ratio = contrast_ratio(ink, paper);
        assert!((ratio - MIN_TEXT_CONTRAST).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn already_passing_lightness_comes_back_untouched() {
        let ink = 0.25;
        let paper = 1.0;
        // 1.05 / 0.30 = 3.5 >= 3.0
        assert_eq!(ensure_contrast(ink, paper, 3.0), ink);
    }

    #[test]
    fn light_paper_darkens_the_ink() {
        // 0.6 on 1.0 reads at ~1.6:1 — the guard must walk the ink down to
        // ~0.20, where 1.05 / 0.25 passes the 4.0 bar.
        let l = ensure_contrast(0.6, 1.0, 4.0);
        assert!(contrast_ratio(l, 1.0) >= 4.0, "ratio with L={l}");
        assert!(l < 0.6);
    }

    #[test]
    fn dark_paper_lightens_the_ink() {
        // 0.3 on 0.35 reads at ~1.1:1 — the guard must walk the ink up to
        // ~0.92, where 0.97 / 0.40 passes the 2.4 bar.
        let l = ensure_contrast(0.3, 0.35, 2.4);
        assert!(contrast_ratio(l, 0.35) >= 2.4, "ratio with L={l}");
        assert!(l > 0.3);
    }

    #[test]
    fn the_walk_stops_at_the_extremes_instead_of_looping() {
        assert!(ensure_contrast(1.0, 0.3, 100.0) <= 1.0);
        assert!(ensure_contrast(0.0, 1.0, 100.0) >= 0.0);
    }
}
