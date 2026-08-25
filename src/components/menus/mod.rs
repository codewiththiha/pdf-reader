//! App menus, built on the floating-system primitives (MenuPopover wrapper).
//!
//! Deprecated transitional shims keep the pre-Phase-6 module paths
//! compiling for in-flight branches.

pub mod app_menu;
pub mod appearance_menu;

/// Deprecated: use `menus::app_menu`.
#[deprecated(note = "use menus::app_menu")]
#[allow(unused_imports)]
pub mod more_menu {
    pub use super::app_menu::*;
}

/// Deprecated: use `menus::appearance_menu`.
#[deprecated(note = "use menus::appearance_menu")]
#[allow(unused_imports)]
pub mod appearance {
    pub use super::appearance_menu::*;
}
