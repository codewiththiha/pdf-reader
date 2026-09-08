//! The library feature: the `/` route page, the shelf it shows, and the surfaces
//! that fill it.
//!
//! Grouped by what a reader does rather than by file count:
//!
//!   * [`page`] — the route: the title bar's three slots and the modal hosts
//!   * [`content`] — the state the page is in (opening, failed, shelf) and the
//!     one order every view below it renders
//!   * [`grid`] / [`list`] / [`shelf_tile`] / [`book_card`] — the shelf itself
//!   * [`add_card`] / [`add_menu`] / [`empty_state`] — the two ways in
//!   * [`import_modal`] — what a folder import is allowed to be
//!   * [`remove_modal`] — what a removal costs, itemised
//!   * [`selection`] — the hold that starts a multi-select, and the bar that acts
//!     on it
//!   * [`progress_dock`] — the run's ring, in the corner
//!   * [`view_menu`] / [`breadcrumb`] / [`titlebar_search`] — the bar's three jobs
//!   * [`drag`] — what a card carries and what a target reads
//!
//! None of these decides anything: the rules live in `library_core` and the
//! operations in `crate::services::library`, so a view here can be replaced
//! without the library's behaviour moving with it.

pub mod add_card;
pub mod add_menu;
pub mod book_card;
pub mod breadcrumb;
pub mod content;
pub mod drag;
pub mod empty_state;
pub mod grid;
pub mod import_modal;
pub mod list;
pub mod page;
pub mod progress_dock;
pub mod remove_modal;
pub mod selection;
pub mod shelf_tile;
pub mod titlebar_search;
pub mod view_menu;

pub use page::LibraryPage;
