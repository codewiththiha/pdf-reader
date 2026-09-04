//! Texture mode -> the two custom properties the textures stylesheet
//! resolves. Shared by design: `.tx-page::before` and `.pdf-page::before`
//! consume the same variables, and the pitch / opacity dials feed both.
//!
//! The `texture-*` page-host class is NOT emitted here — the hosts build
//! it from [`TextureMode::as_str`](crate::appearance::TextureMode::as_str)
//! themselves, because the class rides on their own element rather than on
//! `<html>`.

use crate::appearance::Appearance;

/// `--texture-opacity` and `--texture-scale-user`, written on `<html>` once
/// per appearance change. The stylesheet multiplies the user pitch into the
/// page's own `--scale-factor`, so the pattern zooms with the document.
pub fn css_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    vec![
        (
            "--texture-opacity",
            format!("{:.3}", a.texture_opacity as f64 / 100.0),
        ),
        (
            "--texture-scale-user",
            format!("{:.3}", a.texture_scale as f64 / 100.0),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pair_is_emitted_as_unit_fractions() {
        let a = Appearance { texture_opacity: 90, texture_scale: 150, ..Default::default() };
        let vars = css_vars(&a);
        assert_eq!(vars[0], ("--texture-opacity", "0.900".to_string()));
        assert_eq!(vars[1], ("--texture-scale-user", "1.500".to_string()));
    }
}
