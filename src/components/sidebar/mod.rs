//! The reader sidebar: the composition shell (`shell.rs`) plus its parts —
//! the chrome header, the book-identity row, the panel switcher, and the
//! host wrappers around the reusable outline/thumbnail panels.

pub mod book_info;
pub mod header;
pub mod outline;
pub mod panel_switcher;
pub mod shell;
pub mod thumbnails;

pub(crate) use shell::Sidebar;
