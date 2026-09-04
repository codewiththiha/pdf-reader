//! The reflowable formats' texture surface.
//!
//! A text/Markdown page is a transparent frame over one continuous backdrop,
//! so a per-page texture rectangle would stamp a page-shaped seam onto its
//! paper in single, two-page and horizontal modes (the stylesheet keeps that
//! `::before` dead for these formats). The texture instead rides the scroller
//! that IS the surface — the paginated shell, the continuous stream, the
//! reflowable strip — as a background-image with `background-attachment:
//! local`: one repeating pattern behind all type, scrolling with it, with no
//! blend or z-index machinery (see `styles/textures.css`, TEXT/MD SCROLLER
//! TEXTURE).
//!
//! These two helpers give each scroller the `texture-*` class it needs and
//! the `--tx-zoom` the stylesheet scales the pitch with. For a PDF document
//! the class is empty: its texture stays per-page, because a raster needs
//! the texture composited over the page's own pixels.

use leptos::prelude::*;

use reader_core::appearance::TextureMode;
use crate::state::{ReaderState, TextureSignal};

/// The `texture-*` class for a reflowable document's scroller, or `""` for a
/// PDF document (whose pages own their texture; the class must not land on
/// the PDF scroller, which paints the chrome surface).
pub fn scroller_texture_class(state: ReaderState) -> Memo<String> {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");
    Memo::new(move |_| {
        if !state.reflowable() {
            return String::new();
        }
        let t = texture.get();
        if t == TextureMode::None {
            String::new()
        } else {
            format!("texture-{}", t.as_str())
        }
    })
}

/// The scroller's inline `--tx-zoom`: the same live display scale the type
/// resolves through (`--ts` on the hosts), so the texture pitch zooms in
/// lockstep with the text. `1` for PDF, whose pages carry their own scale
/// factor.
pub fn scroller_zoom_style(state: ReaderState) -> Signal<String> {
    let display = state.viewer.zoom.display;
    Signal::derive(move || {
        let zoom = if state.reflowable() { display.get() } else { 1.0 };
        format!("--tx-zoom:{zoom};")
    })
}
