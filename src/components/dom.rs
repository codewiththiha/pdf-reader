//! The one generic DOM lookup shared across components: an element by id.
//!
//! `by_id` used to live in `components/pdf/dom` (inherited from the old
//! viewer crate), but window chrome needs the same lookup and chrome must
//! not depend on the PDF layer. It lives here; `pdf::dom` re-exports it so
//! the PDF-specific helpers keep working unchanged.

/// The element with `id`, if the document is available and it exists.
pub fn by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}
