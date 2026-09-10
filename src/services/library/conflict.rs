//! The \"already imported?\" sheet: one question, three answers.
//!
//! The RULE is not here — it is `library_core::conflict`, which is pure and
//! host-tested, and answers the only question this surface asks: does the level
//! this arrival is going to already hold a book of this name? This file is the
//! wiring between that answer and the three things a reader can do about it,
//! and it is thin on purpose. It renders nothing (that is
//! `crate::features::library::conflict_modal`), decides nothing (that is the
//! crate) and stores nothing (that is `crate::state::library`).
//!
//! ## The three answers
//!
//! *Already imported* places nothing and reveals the row that is already there
//! — the answer that means \"I did not intend to add anything\", and the one
//! that used to be a book silently vanishing into the shelf it was dropped on.
//! *Add as new* places the arrival under the next free name, so both rows are
//! books and each is a book of its own. *Make link* places a pointer row
//! ([`library_core::book::Row::Link`]) instead of a copy: a row on this shelf
//! that opens the book wherever it lives, holds no fingerprint, no resume
//! point and no highlights, and is invisible to every content check the
//! library runs.
//!
//! ## Two doors
//!
//! [`screen`] splits a batch into the arrivals that may land now and the
//! questions the level has to ask, and [`raise`] puts those questions in front
//! of the reader — the first on screen and the rest waiting behind it. Every
//! placing surface walks through the pair, a drag of four books and a drop of
//! one file alike, which is why an import and a drag cannot disagree about
//! what a collision is: they build the same [`Arrival`] and hand it to the
//! same rule.
//!
//! ## One question at a time
//!
//! The sheet holds a single [`ConflictAsk`] and the rest of a batch waits on
//! [`crate::state::library::LibraryState::conflict_waiting`]: answering pops
//! the next one onto the screen, and Cancel drops them, which is what Cancel
//! has always meant — the placements already answered keep their answers and
//! the ones not asked simply do not land. There is no \"apply to all\" and no
//! second ask, because nothing here is destructive: the worst an answer can do
//! is add a row, and a row is removed by the sheet that says what it takes.

use leptos::prelude::*;

use library_core::book::find_row;
use library_core::conflict::{Answer, Arrival, collide, next_name};

use crate::state::AppState;

/// The question on screen.
#[derive(Clone, PartialEq)]
pub struct ConflictAsk {
    /// The arrival that collided, kept whole: an answer places it, and a
    /// placement needs the file it measured or the row it was moving, the
    /// level it was going to and the slot the drop pointed at.
    pub arrival: Arrival,
    /// The row already on that level whose name the arrival carries — the row
    /// *already imported* reveals and *make link* points at.
    pub existing_id: String,
    /// That row's name, read once: the sheet prints it in three places, and a
    /// heading and two buttons that need one string must not each derive their
    /// own.
    pub existing_name: String,
}

/// Split a batch into the arrivals that may land now and the questions the
/// level has to ask. Every placing surface hands its placements through here
/// BEFORE writing anything — a drag, a lift out to the root, a bulk filing, a
/// loose-file import — and applies the clean half at once, so a drop of ten
/// files with two collisions files eight and asks about two.
pub fn screen(state: AppState, arrivals: Vec<Arrival>) -> (Vec<Arrival>, Vec<ConflictAsk>) {
    let (rows, shelves) = state.library.snapshot_rows();
    let mut clean = Vec::with_capacity(arrivals.len());
    let mut asks = Vec::new();
    for arrival in arrivals {
        match collide(&rows, &shelves, &arrival) {
            Some(existing_id) => {
                let existing_name = find_row(&rows, &existing_id)
                    .map(|row| row.display_name())
                    .unwrap_or_else(|| arrival.name.clone());
                asks.push(ConflictAsk {
                    arrival,
                    existing_id,
                    existing_name,
                });
            }
            None => clean.push(arrival),
        }
    }
    (clean, asks)
}

/// Put questions in front of the reader: the first on screen, the rest waiting
/// behind it. A sheet already up takes them onto its queue rather than being
/// replaced — two drops in flight owe two answers, and a raise that dropped
/// the first question would be a placement vanishing exactly the way this
/// module exists to stop.
pub fn raise(state: AppState, asks: Vec<ConflictAsk>) {
    if asks.is_empty() {
        return;
    }
    let open = state.library.conflict_open.get_untracked();
    if open && state.library.conflict.get_untracked().is_some() {
        state.library.conflict_waiting.update(|waiting| {
            waiting.extend(asks);
        });
        return;
    }
    let mut asks = asks;
    let first = asks.remove(0);
    state.library.conflict_waiting.update(|waiting| {
        waiting.extend(asks);
    });
    state.library.conflict.set(Some(first));
    state.library.conflict_open.set(true);
}

/// One of the three buttons.
pub fn answer(state: AppState, answer: Answer) {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return;
    };
    match answer {
        // Nothing to place: the reader asked to be shown the row they already
        // have, which is the library's own reveal — its shelf, then its card.
        Answer::GoToExisting => super::reveal::reveal_book(state, &ask.existing_id),
        Answer::AsNew => as_new(state, &ask),
        Answer::AsLink => {
            // The link wears the name of the book it points at, which is what
            // makes the row recognisable beside it, and falls back to the
            // arrival's own name if the row went while the sheet was up — a
            // link with no name is a row the shelf cannot label, and
            // `library_core::book::sanitize` drops one.
            let name = state.library.row_name(&ask.existing_id);
            let name = if name.trim().is_empty() {
                ask.arrival.name.clone()
            } else {
                name
            };
            state
                .library
                .add_link(&name, &ask.existing_id, &ask.arrival.shelf_id);
        }
    }
    advance(state);
}

/// Add as new: the arrival takes the next free name on that level and lands.
///
/// A moved row is renamed and then moved — the rename is what frees the
/// collision, and a move that did not rename would ask the same question
/// again on the way in. An imported file is landed under the minted name as
/// its own row, and as a book of its own when the address is one the library
/// already reads ([`library_core::book::Book::independent`]), so the second
/// copy's highlights and its place in it are its own rather than the first
/// one's.
fn as_new(state: AppState, ask: &ConflictAsk) {
    let name = {
        let (rows, shelves) = state.library.snapshot_rows();
        next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
    };
    match &ask.arrival.moving {
        Some(row_id) => {
            state.library.rename_row(row_id, &name);
            super::arrange::move_row(
                state,
                row_id,
                &ask.arrival.shelf_id,
                ask.arrival.index,
            );
        }
        None => {
            let Some(file) = ask.arrival.file.as_ref() else {
                return;
            };
            super::import::land_file(
                state,
                file,
                Some(name),
                &ask.arrival.shelf_id,
                ask.arrival.index,
            );
        }
    }
}

/// The question on screen is answered: the next one up, or the sheet closes.
fn advance(state: AppState) {
    let next = state
        .library
        .conflict_waiting
        .with_untracked(|waiting| waiting.first().cloned());
    match next {
        Some(ask) => {
            state.library.conflict_waiting.update(|waiting| {
                waiting.remove(0);
            });
            state.library.conflict.set(Some(ask));
        }
        None => cancel(state),
    }
}

/// Cancel — the sheet's, the backdrop's and the Escape key's one write. The
/// question on screen and every one behind it are skipped: the placements
/// already answered keep their answers, and the rest simply do not land.
pub fn cancel(state: AppState) {
    state.library.conflict.set(None);
    state.library.conflict_waiting.set(Vec::new());
    state.library.conflict_open.set(false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Book, Fingerprint, Origin, Row};
    use library_core::shelf::Shelf;
    use reader_core::format::Format;

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    /// A Markdown row: the cover queue skips anything that is not a PDF, so a
    /// host test that lands a book never starts the wasm render chain.
    fn row(id: &str, title: &str, path: &str, n: u32) -> Row {
        let mut book = Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked { src: path.to_string() },
            0,
        );
        book.title = Some(title.to_string());
        Row::Book(book)
    }

    fn shelf(id: &str, members: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: Default::default(),
            books: members.iter().map(|m| m.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    fn file(name: &str, n: u32) -> library_core::scan::FoundFile {
        library_core::scan::FoundFile {
            rel: format!("{name}.md"),
            path: format!("/incoming/{name}.md"),
            ext: "md".to_string(),
            size: u64::from(n),
            fp: fp(n),
        }
    }

    /// The state a question is asked of, with the owner that keeps its signals
    /// alive — the pair a test has to hold, because a signal written under a
    /// dropped owner is a signal nobody can read.
    fn library(rows: Vec<Row>, shelves: Vec<Shelf>) -> (Owner, AppState) {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(rows);
        state.library.shelves.set(shelves);
        (owner, state)
    }

    /// The one question a single arrival raises, when it raises one.
    fn the_ask(state: AppState, arrival: Arrival) -> ConflictAsk {
        let (clean, asks) = screen(state, vec![arrival]);
        assert!(clean.is_empty(), "the level has no room for this name");
        asks.into_iter().next().expect("a question")
    }

    #[test]
    fn a_level_with_room_for_the_name_lands_without_asking() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"]), shelf("t", &[])],
        );
        let (clean, asks) = screen(state, vec![Arrival::import(file("Report", 2), "t", None)]);
        assert_eq!(clean.len(), 1);
        assert!(asks.is_empty());
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn a_name_the_level_already_holds_asks_and_names_the_row() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        assert_eq!(ask.existing_id, "b1");
        assert_eq!(ask.existing_name, "Dune");
        // The arrival survives on the ask, because an answer is what places it.
        assert_eq!(ask.arrival.name, "dune");
        assert!(ask.arrival.file.is_some());
    }

    #[test]
    fn a_batch_lands_its_clean_half_and_queues_its_questions() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                row("b2", "Foundation", "/one/foundation.md", 2),
            ],
            vec![shelf("s", &["b1", "b2"])],
        );
        let arrivals = vec![
            Arrival::import(file("neuromancer", 3), "s", None),
            Arrival::import(file("dune", 4), "s", None),
            Arrival::import(file("foundation", 5), "s", None),
        ];
        let (clean, asks) = screen(state, arrivals);
        assert_eq!(clean.len(), 1, "the name nobody holds lands now");
        assert_eq!(clean[0].name, "neuromancer");
        assert_eq!(asks.len(), 2, "and the two that collide ask");
        assert_eq!(asks[0].existing_id, "b1");
        assert_eq!(asks[1].existing_id, "b2");

        raise(state, asks);
        assert!(state.library.conflict_open.get_untracked());
        let on_screen = state.library.conflict.get_untracked().expect("a question");
        assert_eq!(on_screen.existing_id, "b1", "one question at a time");
        assert_eq!(
            state.library.conflict_waiting.get_untracked().len(),
            1,
            "and the rest wait rather than being dropped"
        );

        // A second batch in flight joins the queue behind the one on screen
        // rather than replacing it.
        let (_clean, more) = screen(
            state,
            vec![Arrival::import(file("dune", 6), "s", None)],
        );
        raise(state, more);
        assert_eq!(
            state.library.conflict.get_untracked().map(|a| a.existing_id).as_deref(),
            Some("b1"),
            "the question on screen is still the one that was asked first"
        );
        assert_eq!(state.library.conflict_waiting.get_untracked().len(), 2);

        // Cancel drops what is waiting, which is what Cancel has always meant.
        cancel(state);
        assert!(state.library.conflict.get_untracked().is_none());
        assert!(state.library.conflict_waiting.get_untracked().is_empty());
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn already_imported_places_nothing_and_lights_the_row_it_names() {
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        raise(state, vec![ask]);

        answer(state, Answer::GoToExisting);

        assert_eq!(
            state.library.books.get_untracked().len(),
            1,
            "no row was written"
        );
        assert_eq!(state.library.shelf.get_untracked(), "s");
        let (id, first) = state.library.reveal.get_untracked().expect("a reveal");
        assert_eq!(id, "b1");
        assert!(!state.library.conflict_open.get_untracked());

        // Asking again is asking again: the nonce is what makes a second
        // reveal of the same row a second gesture rather than an equal value
        // nobody is told about.
        let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
        raise(state, vec![ask]);
        answer(state, Answer::GoToExisting);
        let (_, second) = state.library.reveal.get_untracked().expect("a second reveal");
        assert_ne!(first, second);
    }

    #[test]
    fn make_link_puts_a_pointer_on_the_level_and_no_second_book() {
        // The collision IS the row the link will point at, so a link always
        // lands on a level that already holds a book of that name: the reader
        // wants the book reachable from here and does not want a second one.
        let (_owner, state) = library(
            vec![row("b1", "Dune", "/one/dune.md", 1)],
            vec![shelf("s", &["b1"])],
        );
        let ask = the_ask(state, Arrival::import(file("dune", 1), "s", None));
        raise(state, vec![ask]);

        answer(state, Answer::AsLink);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 2, "a link is a row, and not a second book");
        let link = rows.iter().find(|r| r.is_link()).expect("a link");
        assert_eq!(link.display_name(), "Dune", "it wears the name of its book");
        assert_eq!(link.target(), Some("b1"));
        assert_eq!(link.fp(), None, "and has no content identity at all");
        let shelves = state.library.shelves.get_untracked();
        let on_s = shelves.iter().find(|s| s.id == "s").map(|s| s.books.clone());
        assert_eq!(
            on_s,
            Some(vec!["b1".to_string(), link.id().to_string()]),
            "filed on the level it was dropped on, beside the book it points at"
        );
        assert!(!state.library.conflict_open.get_untracked());
    }

    #[test]
    fn a_link_never_blocks_the_next_arrival_and_never_gets_opened() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            ],
            vec![shelf("s", &["l1"])],
        );
        // The only row on the shelf is a pointer: the name is taken by nothing
        // a reader would call a book, so the arrival lands.
        let (clean, asks) = screen(state, vec![Arrival::import(file("dune", 2), "s", None)]);
        assert_eq!(clean.len(), 1);
        assert!(asks.is_empty());
        // And a pointer is not the library's row for a content: the book it
        // points at is, whichever shelf that is filed on.
        assert_eq!(state.library.row("l1").map(|r| r.is_link()), Some(true));
        assert_eq!(state.library.row_name("l1"), "Dune");
        assert_eq!(state.library.row_name("gone"), "");
    }

    #[test]
    fn add_as_new_renames_a_moved_row_and_then_moves_it() {
        let (_owner, state) = library(
            vec![
                row("b1", "Dune", "/one/dune.md", 1),
                row("b2", "Dune", "/two/dune.md", 2),
            ],
            vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
        );
        let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
        raise(state, vec![ask]);

        answer(state, Answer::AsNew);

        let rows = state.library.books.get_untracked();
        let renamed = rows
            .iter()
            .find(|r| r.id() == "b2")
            .map(Row::display_name)
            .unwrap_or_default();
        assert_eq!(renamed, "Dune_1", "the counter is the answer, not a dialog");
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves.iter().find(|s| s.id == "s").map(|s| s.books.clone()),
            Some(vec!["b1".to_string(), "b2".to_string()]),
            "and it landed on the shelf it was dropped on"
        );
        assert_eq!(
            shelves.iter().find(|s| s.id == "t").map(|s| s.books.clone()),
            Some(Vec::new()),
            "having left the one it was lifted from"
        );
        // The name it took is free on the level it left, and the level it
        // joined now holds both names.
        assert_eq!(
            state.library.row_name("b1"),
            "Dune",
            "the row that was already there keeps its own name"
        );
    }
}
