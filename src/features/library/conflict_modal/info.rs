//! Everything the collision sheet prints, read once per answer.
//!
//! A `view!` body is a builder, not a place to compute: a heading and two buttons
//! that need the same string must not each derive their own, and a count taken
//! inside a render closure is a count re-taken on every reactive re-run of the
//! sheet. So the sheet's eleven strings are built here, once, off one snapshot of
//! the library — the same snapshot the click then answers against, which is what
//! stops a row promising one thing and doing another.

use leptos::prelude::*;

use library_core::conflict::next_name;
use library_core::shelf::ALL_SHELF;

use crate::services::library::conflict::{self, ConflictAsk};
use crate::state::AppState;

/// Everything the sheet prints, read once per answer — the remove receipt's
/// rule: a `view!` body is a builder, not a place to compute, and a heading and
/// two buttons that need the same name must not each derive their own.
pub(super) struct NameSheetInfo {
    /// The name arriving — the heading.
    pub(super) incoming: String,
    /// Whether the arrival is a file with no row of its own yet. The sheet's
    /// sentence turns on it — an import has nothing of its own to keep, so its
    /// question is "what do I put here" rather than "which of the two do I keep".
    pub(super) import: bool,
    /// The answers this question offers, in the order the sheet shows them —
    /// `library_core::conflict::Placement`'s own lists, chosen by what is
    /// arriving. The sheet renders these and cannot offer a button the apply has
    /// never heard of, which is what the two used to be able to do when each
    /// spelled the condition out.
    pub(super) offers: &'static [library_core::conflict::Placement],
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
}

/// The level an arrival is going to, as a sheet's sentence says it. One
/// spelling for every question that names the level — the name sheet's, the
/// covered file's — because two sheets that worded the same shelf differently
/// would read as two different places. An empty name means the shelf went
/// while the sheet was up, which is the same answer as the root's: a level
/// with no name to speak.
pub(super) fn where_line(state: AppState, shelf_id: &str) -> String {
    if shelf_id == ALL_SHELF {
        "in your library".to_string()
    } else {
        match state.library.shelf_name(shelf_id) {
            name if !name.is_empty() => format!("on “{name}”"),
            _ => "on this shelf".to_string(),
        }
    }
}

/// A subtitle with the queue's count on it, when there is a queue:
/// "Into “Fiction” · 3 more waiting". One spelling for the three sheets whose
/// questions queue, so the count reads the same whichever question is up.
pub(super) fn more_waiting(subtitle: String, waiting: usize) -> String {
    if waiting > 0 {
        format!("{subtitle} · {} more waiting", waiting)
    } else {
        subtitle
    }
}

impl NameSheetInfo {
    pub(super) fn of(state: AppState, ask: &ConflictAsk) -> Self {
        let where_line = where_line(state, &ask.arrival.shelf_id);
        // One read of both lists, so the promise on the row and the answer the
        // click gives are counted against the same library.
        let (rows, shelves) = state.library.snapshot_rows();
        let new_name = next_name(
            &rows,
            &shelves,
            &ask.arrival.shelf_id,
            &ask.arrival.name,
        );
        // The row's own id, which is the row's own list: a count taken from the
        // address would promise a loss the merge cannot make, because a twin
        // still reading that address keeps its own marks.
        let marks = crate::storage::load_gloss()
            .get(&ask.existing_id)
            .map(Vec::len)
            .unwrap_or(0);
        Self {
            incoming: ask.arrival.name.clone(),
            import: ask.arrival.is_import(),
            offers: conflict::offers_for(state, ask),
            existing_name: ask.existing_name.clone(),
            marks,
            new_name,
            where_line,
            waiting: state.library.conflict_waiting.with_untracked(|w| w.len()),
        }
    }
}
