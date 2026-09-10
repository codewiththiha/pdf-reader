//! The document lifecycle: opening (dialog, path, OS file events, a library
//! row) and closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same entry
//! points, none of which depend on UI.
//!
//! [`gloss_key`] is the one fact the lifecycle owns that is not about the
//! engine: WHICH book the open document is. The address on its own cannot say,
//! because the library may hold two rows of one file, and every reader of the
//! highlights and every writer of a resume point needs the answer.

pub mod close;
pub mod open;
pub(crate) mod session;

pub use close::close_document;
pub use open::{init_open_file_handling, open_book, open_dialog, open_path};

use leptos::prelude::*;

use library_core::book::gloss_key_of;

use crate::state::AppState;

/// The key the open document's highlights are stored under.
///
/// The address, unless the reader opened a ROW the library can name and that
/// row is a book of its own
/// ([`library_core::book::Book::independent`]), whose marks live under a key
/// carrying its id. Every writer of the marks asks here rather than reading
/// the document's path — the load at open
/// (`crate::services::document::open::enter`), the save on every stroke
/// (`crate::components::ai::gloss::controller`) and the sweep on a removal
/// (`crate::services::library::arrange`) — so the three cannot disagree about
/// which list they mean.
///
/// Empty when nothing is open, which every caller reads as "nowhere to put
/// it" rather than as a key.
pub(crate) fn gloss_key(state: AppState) -> String {
    let Some(path) = state.reader.document.path.get_untracked() else {
        return String::new();
    };
    let book_id = state.reader.document.book_id.get_untracked();
    state
        .library
        .books
        .with_untracked(|books| gloss_key_of(books, book_id.as_deref(), &path))
}
