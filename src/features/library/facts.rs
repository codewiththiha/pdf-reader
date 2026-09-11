//! The facts about a book that a shelf surface paints, read back out of the
//! library by id on the frame they are asked for.
//!
//! A card and a row are keyed by id, and a keyed row is NOT re-created when its
//! content changes — a startup measurement marking the book missing, a relink
//! moving its address, a conflict sheet's rename, a fold merging a twin into it.
//! So every fact that can move is read back by the id and the prop supplies the
//! identity alone.
//!
//! One struct and one derive for both densities, because the grid's card and the
//! list's row were reading the same facts back separately and two of them
//! differently: the card decided "the author, else the resume point" at render
//! time and the row decided it at derive time, and nothing kept the two agreeing
//! about the same book. [`BookFacts::author_line`] is that decision made once.

use leptos::prelude::*;

use library_core::book::find_by_id;
use library_core::text::page_line;

use crate::state::AppState;

/// What a shelf surface paints about one book.
#[derive(Clone)]
pub(crate) struct BookFacts {
    /// Where the book lives. The line a card falls back to when a title the
    /// document supplied gives the reader no way to tell two books called
    /// "Report" apart — and the key the cover cache answers to, which is why a
    /// relink has to move it: the old address's art belongs to nobody afterwards.
    pub path: String,
    pub title: String,
    /// The author, when the book has one worth showing.
    pub author: Option<String>,
    /// The one line of prose a row has room for: the author when the book has
    /// one and the resume point when it does not.
    pub author_line: String,
    /// "Page 12 of 340" — the card's fallback line, with the address as its
    /// tooltip.
    pub page_line: String,
    /// Reading progress as a fraction, when the book has reported a length.
    pub progress: Option<f64>,
    /// The address stopped resolving, so the surface greys the row and offers a
    /// relink.
    pub missing: bool,
}

impl BookFacts {
    /// The progress as the percentage a row prints beside its title. The card
    /// draws [`Self::progress`] as a bar instead; both read one measurement.
    pub(crate) fn percent(&self) -> Option<String> {
        self.progress.map(|p| format!("{:.0}%", p * 100.0))
    }
}

/// Read one book's facts back out of the library, reactively.
///
/// One derive rather than one per field: the facts all move together — a relink
/// rewrites the address AND the art it keys on — and a surface that read six
/// signals would subscribe six times to one list. `None` is the beat between a
/// removal and the list catching up, where the surface paints its blanks and is
/// gone next tick.
pub(crate) fn book_facts(state: AppState, book_id: &str) -> Signal<Option<BookFacts>> {
    let id = book_id.to_string();
    Signal::derive(move || {
        state.library.books.with(|rows| {
            find_by_id(rows, &id).map(|b| BookFacts {
                path: b.path().to_string(),
                title: b.title(),
                author_line: b.author().unwrap_or_else(|| page_line(b.page, b.num_pages)),
                author: b.author(),
                page_line: page_line(b.page, b.num_pages),
                progress: b.progress(),
                missing: b.missing,
            })
        })
    })
}
