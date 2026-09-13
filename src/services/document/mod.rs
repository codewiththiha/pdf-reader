//! The document lifecycle: opening (dialog, path, OS file events, a library
//! row) and closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same entry
//! points, none of which depend on UI.
//!
//! [`gloss_key`] is the one fact the lifecycle owns that is not about the
//! engine: WHICH book the open document is. The address on its own cannot say,
//! because the library may hold two rows of one file, and every reader of the
//! highlights and every writer of a resume point needs the answer. The answer is
//! the row's ID rather than a string derived from its address, which is what
//! lets a conversion or a move keep the marks without carrying them anywhere.

pub mod close;
pub mod open;
pub(crate) mod session;

pub use close::close_document;
pub use open::{init_open_file_handling, open_dialog, open_path, open_row};

use leptos::prelude::*;

use crate::state::AppState;

/// The key the open document's highlights are stored under: the id of the row the
/// library holds for it.
///
/// An id and not a string derived from the address, which is what the marks used
/// to be keyed by. The derived key was the reason two functions existed purely to
/// compensate for it — a Linked→Stored conversion changed the address and so had
/// to carry the marks across by hand, and a merge had to read both rows' keys,
/// union the lists and write the result back. An id does not move when the bytes
/// do, so neither does the key, and neither compensation has anything left to
/// compensate for.
///
/// It is also why two rows of one file no longer need a special key to keep their
/// marks apart: each row has its own id, so each has its own list, and "removing
/// one of the two takes nothing from the other" is a property of the storage
/// rather than a rule every remover has to remember.
///
/// Every writer of the marks asks here rather than reading the document's path —
/// the load at open (`crate::services::document::open::enter`), the save on every
/// stroke (`crate::components::ai::gloss::controller`) and the sweep on a removal
/// (`crate::services::library::arrange`) — so the three cannot disagree about
/// which list they mean.
///
/// Empty when nothing is open or the open has no row the library can name, which
/// every caller reads as "nowhere to put it" rather than as a key. An open that
/// arrived as nothing but an address settles onto its row before any tail loads
/// the marks (`crate::services::document::open`), so the window where this is
/// empty is the window where there is no book to have marks about.
pub(crate) fn gloss_key(state: AppState) -> String {
    state
        .reader
        .document
        .book_id
        .get_untracked()
        .unwrap_or_default()
}
