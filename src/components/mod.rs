//! App UI, organized by what a component belongs to: shared primitives,
//! window chrome, menus, overlays, reader controls, and the sidebar.
//! The reusable atoms + viewer components live in `pdf-viewer`.

pub mod chrome;
pub mod menus;
pub mod overlays;
pub mod reader;
pub mod shared;
pub mod sidebar;

pub use sidebar::Sidebar;
