//! Everything the collision sheet prints, read once per answer.
//!
//! A `view!` body is a builder, not a place to compute: a heading and two buttons
//! that need the same string must not each derive their own, and a count taken
//! inside a render closure is a count re-taken on every reactive re-run of the
//! sheet. So the sheet's eleven strings are built here, once, off one snapshot of
//! the library — the same snapshot the click then answers against, which is what
//! stops a row promising one thing and doing another.

use leptos::prelude::*;

use library_core::book::find_row;
use library_core::conflict::next_name;
use library_core::shelf::ALL_SHELF;

use crate::services::library::conflict::ConflictAsk;
use crate::state::AppState;

/// Everything the sheet prints, read once per answer — the remove receipt's
/// rule: a `view!` body is a builder, not a place to compute, and a heading and
/// two buttons that need the same name must not each derive their own.
pub(super) struct Info {
    /// The name arriving — the heading.
    pub(super) incoming: String,
    /// Whether the arrival is a file with no row of its own yet, which is the
    /// fact that decides which three rows the sheet offers.
    pub(super) import: bool,
    /// The name already on the level, which *already imported* goes to and
    /// *make link* points at.
    pub(super) existing_name: String,
    /// How many highlights the row already here holds — what a Replace takes
    /// with it, promised on its own row rather than asked about afterwards.
    pub(super) marks: usize,
    /// The name *add as new* would mint, promised on its own row: "keep both"
    /// without the name is an answer the reader has to take on faith.
    pub(super) new_name: String,
    /// Where the row that collided is: a shelf the sentence can name, or the
    /// library's own unfiled list, which has no name but "your library".
    pub(super) where_line: String,
    /// How many questions wait behind this one.
    pub(super) waiting: usize,
    /// Whether the move is the pointer shape: the row being dragged is a
    /// read-at-place book an in-place folder placed, and the row on the level
    /// is one of the library's own stored copies. Neither side is the
    /// reader's to destroy, so the sheet offers *make link* in place of the
    /// destructive *replace*: reach the copy from here, and keep both the
    /// file on disk and the bytes in the store exactly as they are.
    pub(super) link_offer: bool,
}

impl Info {
    pub(super) fn of(state: AppState, ask: &ConflictAsk) -> Self {
        let where_line = if ask.arrival.shelf_id == ALL_SHELF {
            "in your library".to_string()
        } else {
            // Empty means the shelf went while the sheet was up, which is
            // the same answer as the root's: a level with no name to speak.
            // `sanitize` drops a shelf whose name is blank, so nothing on a
            // loaded list answers with one.
            match state.library.shelf_name(&ask.arrival.shelf_id) {
                name if !name.is_empty() => format!("on “{name}”"),
                _ => "on this shelf".to_string(),
            }
        };
        // One read of both lists, so the promise on the row and the answer the
        // click gives are counted against the same library.
        let (rows, shelves) = state.library.snapshot_rows();
        let new_name = next_name(
            &rows,
            &shelves,
            &ask.arrival.shelf_id,
            &ask.arrival.name,
        );
        // The row's OWN key, not its address: a book of its own keeps its
        // marks under a key of its id, and a count taken from the address would
        // promise a loss the removal cannot make.
        let marks = find_row(&rows, &ask.existing_id)
            .and_then(|row| row.book())
            .map(|book| {
                crate::storage::load_gloss()
                    .get(&book.gloss_key())
                    .map(Vec::len)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        // The pointer shape is a fact about the two ROWS, read off the same
        // snapshot the rest of the sheet counts against.
        let link_offer = ask.arrival.moving.as_ref().is_some_and(|moved_id| {
            find_row(&rows, &ask.existing_id)
                .and_then(|row| row.book())
                .is_some_and(|book| book.origin.is_stored())
                && crate::services::library::arrange::converts_on_move_to(
                    state,
                    moved_id,
                    &ask.arrival.shelf_id,
                )
        });
        Self {
            incoming: ask.arrival.name.clone(),
            import: ask.arrival.is_import(),
            existing_name: ask.existing_name.clone(),
            marks,
            new_name,
            where_line,
            waiting: state.library.conflict_waiting.with_untracked(|w| w.len()),
            link_offer,
        }
    }
}
