//! Application effects: app-level concerns (appearance, drag-drop, links,
//! reading progress, page selection, theme) plus the viewer effects that
//! previously lived in the `pdf-viewer` crate (fit/zoom, scroll, page
//! tracking, search, shortcuts).

pub mod appearance;
pub mod continuous_scroll;
pub mod drag_drop;
pub mod fit;
pub mod link_navigation;
pub mod page_selection;
pub mod page_tracking;
pub mod reading_progress;
pub mod search_effects;
pub mod shortcuts;
pub mod theme;
pub mod zoom;
