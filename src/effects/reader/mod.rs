//! Reader effects: the reactive systems that keep the reader in sync
//! (scroll/page navigation sync, fit/zoom, search, selection tracking,
//! reading progress).

pub mod auto_scroll;
pub mod vertical_scroll_sync;
pub mod fit_mode;
pub mod link_navigation;
pub mod on_mode_change;
pub mod navigation_sync;
pub mod page_selection;
pub mod reading_progress;
pub mod search;
pub mod text_selection;
