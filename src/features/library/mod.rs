//! The library feature: the `/` route page, the shelf it shows, and the surfaces
//! that fill it.
//!
//! Grouped by what a reader does rather than by file count:
//!
//!   * [`page`] — the route: the title bar's three slots, the modal hosts, and the
//!     drag session with the layer that draws what it is holding
//!   * [`content`] — the state the page is in (opening, failed, shelf) and the one
//!     order every view below it renders
//!   * [`grid`] / [`list`] / [`folder_card`] / [`book_card`] — the shelf itself
//!   * [`gestures`] — the press contract every one of those items shares: one
//!     wrapper's decision, the drag session's endpoints, the keyboard's halves
//!     and the right-click's ask, wired once
//!   * [`add_card`] / [`add_menu`] / [`empty_state`] — the two ways in
//!   * [`import_modal`] — what a folder import is allowed to be
//!   * [`remove_modal`] — what a removal costs, itemised
//!   * [`selection`] — the hold that starts a multi-select, and the bar that acts
//!     on it
//!   * [`context_menu`] — the right-click: one menu per kind of thing under the
//!     pointer, and the one host every surface asks
//!   * [`progress_dock`] — the run's ring, in the corner
//!   * [`view_menu`] / [`breadcrumb`] / [`titlebar_search`] — the bar's three jobs
//!   * [`dnd`] — the drag that moves things: the session, the targets it can land
//!     on, the table that decides what a drop means, and the overlay it draws
//!
//! A level of the library is the same shape at every depth: [`grid`] renders the
//! folders filed at this level and then the books on it, and a folder card is one
//! cell of the grid exactly like a book's — which is what lets a shelf be filed
//! inside another one. [`content`] derives the level once and provides both halves.
//!
//! None of these decides anything: the rules live in `library_core` and the
//! operations in `crate::services::library`, so a view here can be replaced
//! without the library's behaviour moving with it. [`dnd`] is the exception that
//! proves the shape — it decides what a DROP means, which is a question about a
//! gesture rather than about the library, and it hands the answer to the same
//! services every menu row uses.

pub mod add_card;
pub mod add_menu;
pub mod book_card;
pub mod breadcrumb;
pub mod content;
pub mod context_menu;
pub mod dnd;
pub mod empty_state;
pub mod folder_card;
pub mod gestures;
pub mod grid;
pub mod import_modal;
pub mod list;
pub mod page;
pub mod progress_dock;
pub mod remove_modal;
pub mod selection;
pub mod titlebar_search;
pub mod view_menu;

pub use page::LibraryPage;
