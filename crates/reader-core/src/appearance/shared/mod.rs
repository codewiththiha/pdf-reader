//! The appearance kernel's shared helpers: the colour-space maths, the
//! tint hue mapping and the noise/texture emitters. Both the PDF pipeline
//! (`appearance_pdf`) and the text pipeline (`appearance_text`) consume
//! these; neither format owns them.

pub mod noise;
pub mod oklch;
pub mod texture;
pub mod tint;
