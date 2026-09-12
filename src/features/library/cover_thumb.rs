//! The library's one cover painter: the cached art for an address, or the
//! fallback the calling surface names.
//!
//! The grid's card, the list's row and the search bar's thumb each
//! hand-rolled the same subscription to the cover cache — the same `get` on
//! the same signal, the same `img` with the same lazy-loading and the same
//! no-drag, and a fallback each surface spelled itself. A cache that changed
//! shape (a relink moves the key, a prune drops the entry) was a change in
//! every one of them. The folder plate's cell keeps its own read, and the
//! reason is its wrapper: the cell styles ITSELF by whether the art is there,
//! which is a question about the cell and not about the `img`.

use leptos::prelude::*;

use crate::state::AppState;

/// The cover for one address: the cached art when there is any, and the
/// surface's own fallback when there is not.
#[component]
pub(crate) fn CoverThumb(
    state: AppState,
    /// The address the cache answers to, read on the frame it is asked for:
    /// a relink moves a book's art key and the surface has to follow. Empty
    /// (the beat between a removal and the list catching up) paints the
    /// fallback, which is what the surfaces painted for a book with no facts.
    path: Signal<String>,
    /// The art's alt text — the book's own name on the surfaces that have
    /// one, and nothing on the ones that are decoration.
    alt: Signal<String>,
    /// The class the surface's own CSS gives the art.
    img_class: &'static str,
    /// What paints while the cache has no art. `None` paints nothing.
    #[prop(optional)]
    fallback: Option<ChildrenFn>,
) -> impl IntoView {
    view! {
        {move || {
            match state
                .library
                .covers
                .with(|covers| covers.get(&path.get()).cloned())
            {
                Some(cover) => {
                    view! {
                        // Not draggable, and the reason is the whole of a
                        // shelf card's gesture: an image is natively
                        // draggable, so a press on the cover would hand the
                        // pointer to the engine's own drag, which is the drag
                        // this shelf no longer uses and the one that used to
                        // swallow the release.
                        <img
                            class=img_class
                            src=cover.data_url.clone()
                            alt=alt.get()
                            loading="lazy"
                            draggable="false"
                        />
                    }
                        .into_any()
                }
                None => fallback
                    .as_ref()
                    .map(|f| f())
                    .unwrap_or_else(|| ().into_any()),
            }
        }}
    }
}
