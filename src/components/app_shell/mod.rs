//! Reusable structural application shell: the title bar shell, the document
//! titles, the popovers the toolbar opens, and the geometry metrics that Rust
//! layout code shares with the CSS classes.

pub mod app_title_bar;
pub mod document_title;
pub mod floating_document_title;
pub mod title_bar;
pub mod toolbar_popover;
pub mod traffic_lights;
pub(crate) mod constants;
