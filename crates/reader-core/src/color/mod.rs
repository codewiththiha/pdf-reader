//! Colour spaces the reader computes in.
//!
//! `oklch` is the perceptual space the tint pipeline works in: a hue slider and
//! a strength slider there move a colour the way the eye reads them, which is
//! what lets one accent derive a whole theme's accents without a table of
//! hand-picked pairs.
pub mod oklch;

pub use oklch::{hex_to_oklch, oklch_css};
