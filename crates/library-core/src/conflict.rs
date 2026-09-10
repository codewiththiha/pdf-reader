//! The one question the library asks about a name: does the level this is
//! going to already hold a book called that?
//!
//! Nothing else about duplicates is a dialog. A second copy of one file is a
//! naming problem with a naming answer — the counter a file manager appends —
//! and a reader who wants a pointer rather than a copy gets a row that points
//! ([`crate::book::Row::Link`]). So this module is one predicate
//! ([`collide`]), one namer ([`next_name`]) and the three answers
//! ([`Answer`]) the sheet offers when the predicate says yes.
//!
//! ## Why names and not content
//!
//! The question used to be asked of a file's fingerprint, which made it a
//! question about bytes: two rows of one file shared an address, and with it
//! their highlights, their resume point and their removal, so keeping both
//! meant two books that could not be told apart by anything the reader could
//! see. Names are what a shelf is: a reader looking at two rows called `1` and
//! `1_1` knows they are two books, and a reader looking at two rows both
//! called `1` does not. So the collision is a name collision, on the level the
//! arrival is going to, and everything the fingerprint used to decide is now
//! either a naming decision ([`next_name`]) or a pointer ([`Answer::AsLink`]).
//!
//! Fingerprints are not gone from the library — a watched folder's rescan
//! ([`crate::ledger`]) and a path check ([`crate::book::apply_check`]) still
//! need one, because "is this the file I already placed" is a question about
//! bytes and only bytes can answer it. They are gone from this module, which
//! asks a different question.
//!
//! ## What never asks
//!
//! A link, on either side: it is not a book, so it is never a collision and
//! never the thing collided with. The row being moved itself, because a
//! reorder is not an arrival. A level that holds no book of that name, which
//! includes an empty folder and includes the shelf holding `1_1` when `1`
//! arrives — different names are different books, and a counter a previous
//! answer minted is not a reason to ask again.
//!
//! Pure, and host-tested: no state, no signals, no storage, so the four rules
//! above are assertions rather than behaviours.

use std::collections::HashSet;

use crate::book::{Row, duplicate_title, find_row, stem_of};
use crate::scan::FoundFile;
use crate::shelf::Shelf;

/// What is arriving, and where it is going.
///
/// One value for both routes in, which is the reason an import and a drag can
/// no longer disagree about what a collision is: they build the same arrival
/// and hand it to the same [`collide`].
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the shelf would show it: a file's stem for an
    /// import, the moved row's own name for a drag. This is what [`collide`]
    /// compares, so it is never a file name with an extension on it — a title
    /// that looks like a file name is a title [`crate::book::sanitize`] drops.
    pub name: String,
    /// The row being moved, when this is a move. It cannot collide with
    /// itself, and a link row's id is as valid here as a book's.
    pub moving: Option<String>,
    /// The file being imported, when this is an import: the address and the
    /// measurement the row is minted from. A move has none — its row already
    /// exists.
    pub file: Option<FoundFile>,
    /// The level the arrival is going to; [`ALL_SHELF`] for the root.
    pub shelf_id: String,
    /// The slot the drop pointed at; `None` appends. Carried rather than
    /// re-derived because an arrival that had to ask lands later, when the
    /// level it was aimed at may have moved.
    pub index: Option<usize>,
}

impl Arrival {
    /// A file being imported onto a level.
    pub fn import(file: FoundFile, shelf_id: impl Into<String>, index: Option<usize>) -> Self {
        let name = stem_of(&file.path);
        Self {
            name,
            moving: None,
            file: Some(file),
            shelf_id: shelf_id.into(),
            index,
        }
    }

    /// A row being moved or filed onto a level. `name` is the row's own
    /// [`Row::display_name`], read by the caller because the caller is the one
    /// holding the row list.
    pub fn moved(
        row_id: impl Into<String>,
        name: impl Into<String>,
        shelf_id: impl Into<String>,
        index: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            moving: Some(row_id.into()),
            file: None,
            shelf_id: shelf_id.into(),
            index,
        }
    }

    /// Whether this arrival is a file with no row of its own yet.
    pub fn is_import(&self) -> bool {
        self.file.is_some()
    }
}

/// The reader's answer to a name collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Place nothing: take the reader to the row that is already there,
    /// wherever in the library it is filed.
    GoToExisting,
    /// Place it under the next free name — `1` → `1_1` → `1_2` — so both rows
    /// are books and each is a book of its own
    /// ([`crate::book::Book::independent`]).
    AsNew,
    /// Place a [`Row::Link`] pointing at the row that is already there: a row
    /// on this shelf that is not a second copy of anything.
    AsLink,
}

/// Whether two names are the same name. Case-insensitive and nothing else: a
/// shelf is read by a person, and `Report` beside `report` is two rows of one
/// name however the filesystem would have spelled them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The row already on the target level whose name this arrival carries, when
/// there is one.
///
/// The whole rule, and all of it:
///
///   * only BOOK rows are compared — a link is not a book and has no name of
///     its own to defend, so it never collides and never blocks;
///   * only rows on the TARGET level, which is a shelf's own member list, and
///     at the root the rows nobody has filed (the list \"All\" renders — the
///     root is a level like any other and a drop on Home beside an unfiled row
///     of one name is the same question as a drop on a shelf beside a filed
///     one);
///   * the row being moved is never its own collision;
///   * names compare as [`same_name`], so a counter a previous answer minted
///     (`1_1`) is a different name from the one that arrives (`1`).
pub fn collide(rows: &[Row], shelves: &[Shelf], at: &Arrival) -> Option<String> {
    level_members(rows, shelves, &at.shelf_id)
        .into_iter()
        .find_map(|member| {
            let row = find_row(rows, member)?;
            if row.is_link() || Some(row.id()) == at.moving.as_deref() {
                return None;
            }
            same_name(&row.display_name(), &at.name).then(|| row.id().to_string())
        })
}

/// The next free name for `name` on one level: `1` → `1_1` → `1_2`, the
/// counter a file manager appends, counted against the names that level
/// already shows rather than against the whole library.
///
/// A level's own names are the right pool because the collision was a level's:
/// `1` on \"Fiction\" and `1` on \"Sci-Fi\" are two rows a reader never sees
/// together, and renaming the second of them would be an answer to a question
/// nobody asked. The counter itself is [`duplicate_title`]'s — it steps rather
/// than stacks (`1_1` becomes `1_2`, not `1_1_1`), it fills gaps, and the name
/// it mints survives [`crate::book::sanitize`]'s rule about titles that look
/// like file names.
pub fn next_name(rows: &[Row], shelves: &[Shelf], shelf_id: &str, name: &str) -> String {
    let in_use: HashSet<String> = level_members(rows, shelves, shelf_id)
        .iter()
        .filter_map(|member| find_row(rows, member))
        .map(Row::display_name)
        .collect();
    duplicate_title(name, &in_use)
}

/// The ids of the rows one level holds — [`crate::shelf::members_of`], which
/// is the one answer to that question rather than this module's own: a shelf's
/// member list, and at the root the rows no shelf holds. A shelf id that names
/// no shelf and is not the root answers with nothing, so an arrival aimed at a
/// level that does not exist has nothing to collide with and the placement
/// that follows is the caller's to refuse.
fn level_members<'a>(rows: &'a [Row], shelves: &'a [Shelf], shelf_id: &str) -> Vec<&'a str> {
    crate::shelf::members_of(rows, shelves, shelf_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Fingerprint, Origin};
    use crate::shelf::ALL_SHELF;
    use reader_core::format::Format;

    fn book(id: &str, path: &str) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            Fingerprint { size: 10, mtime_ms: 5, head_hash: 9 },
            Format::Pdf,
            Origin::Linked { src: path.to_string() },
            10,
        ))
    }

    /// A book row that shows `title`, which is what a collision compares.
    fn titled(id: &str, path: &str, title: &str) -> Row {
        let mut row = book(id, path);
        row.as_book_mut().unwrap().title = Some(title.to_string());
        row
    }

    fn link(id: &str, name: &str, target: &str) -> Row {
        Row::link(id.to_string(), name.to_string(), target.to_string(), 10)
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

    fn file(name: &str) -> FoundFile {
        FoundFile {
            rel: name.to_string(),
            path: format!("/books/{name}"),
            ext: "pdf".to_string(),
            size: 10,
            fp: Fingerprint { size: 10, mtime_ms: 5, head_hash: 9 },
        }
    }

    /// An import of `name.pdf` onto `shelf_id`.
    fn import(name: &str, shelf_id: &str) -> Arrival {
        Arrival::import(file(&format!("{name}.pdf")), shelf_id, None)
    }

    /// A drag of one row onto `shelf_id`.
    fn drag(row_id: &str, name: &str, shelf_id: &str) -> Arrival {
        Arrival::moved(row_id, name, shelf_id, None)
    }

    // -------------------------------------------------------------------
    // The four rules the question is made of.
    // -------------------------------------------------------------------

    #[test]
    fn a_name_already_on_the_shelf_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")).as_deref(),
            Some("b1")
        );
        // And the same name spelled the way the other filesystem would.
        let named = vec![titled("b1", "/books/report.pdf", "Report")];
        assert_eq!(
            collide(&named, &shelves, &import("report", "s")).as_deref(),
            Some("b1"),
            "a shelf is read by a person, not by a byte comparison"
        );
    }

    #[test]
    fn an_empty_shelf_never_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &[]), shelf("t", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1", "s")), None);
        // Nothing in the library at all: the first file in is never a
        // duplicate of itself.
        let empty: Vec<Row> = Vec::new();
        assert_eq!(collide(&empty, &shelves, &import("1", "s")), None);
        // A shelf id that names no shelf is not a level with members in it.
        assert_eq!(collide(&rows, &shelves, &import("1", "gone")), None);
    }

    #[test]
    fn a_counter_copy_beside_its_original_never_asks() {
        // The answer to a first collision is a name, and a name that was
        // minted is not a reason to ask again: `1_1` arriving beside `1` is a
        // second book, and dragging it in beside the row it was named from is
        // the move the reader meant.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
        ];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &drag("b2", "1_1", "s")), None);
        // And the original still asks, which is the pair of facts that makes
        // the counter mean something.
        assert_eq!(collide(&rows, &shelves, &import("1", "s")).as_deref(), Some("b1"));
    }

    #[test]
    fn a_link_neither_asks_nor_blocks() {
        // A link is not a book: it carries the name of the book it points at,
        // and a second row of that name arriving is not a collision with a
        // pointer. Nor is a pointer the thing a collision is found against.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            link("l1", "1", "b1"),
        ];
        let shelves = vec![shelf("s", &["l1"]), shelf("t", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")),
            None,
            "the only row on this shelf is a pointer"
        );
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "t")).as_deref(), Some("b1"));
        // A link never collides with itself either, and a link moved onto the
        // shelf holding its own book is the quiet case the row exists for.
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "s")), None);
    }

    // -------------------------------------------------------------------
    // The rest of the rule.
    // -------------------------------------------------------------------

    #[test]
    fn a_row_never_collides_with_itself() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &[])];
        assert_eq!(collide(&rows, &shelves, &drag("b1", "1", "s")), None);
        // A reorder inside its own shelf is the same fact with an index.
        let mut reorder = drag("b1", "1", "s");
        reorder.index = Some(0);
        assert_eq!(collide(&rows, &shelves, &reorder), None);
    }

    #[test]
    fn a_twin_on_another_shelf_is_not_this_shelfs_question() {
        // The collision is a NAME on a LEVEL. A book called `1` on Fiction says
        // nothing about a `1` arriving on Sci-Fi: the two are never on screen
        // together, and the reader who filed them there filed two books.
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("fiction", &["b1"]), shelf("scifi", &[])];
        assert_eq!(collide(&rows, &shelves, &import("1", "scifi")), None);
    }

    #[test]
    fn the_root_is_a_level_and_its_list_is_the_unfiled_rows() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/books/2.pdf", "2"),
        ];
        let shelves = vec![shelf("s", &["b2"])];
        // b1 is unfiled, so it is on the root's list and a drop on Home asks.
        assert_eq!(
            collide(&rows, &shelves, &import("1", ALL_SHELF)).as_deref(),
            Some("b1")
        );
        // b2 is filed, so it is not on that list, and Home is where a second
        // row of it lands as its own book.
        assert_eq!(collide(&rows, &shelves, &import("2", ALL_SHELF)), None);
    }

    #[test]
    fn the_counter_counts_against_the_level_it_lands_on() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
            titled("b3", "/elsewhere/1.pdf", "1"),
        ];
        let shelves = vec![shelf("s", &["b1", "b2"]), shelf("t", &["b3"])];
        // On s, `1` and `1_1` are taken, so the next free counter is `1_2`.
        assert_eq!(next_name(&rows, &shelves, "s", "1"), "1_2");
        // On t only `1` is, so the same arrival is `1_1` there: the pool is the
        // level's, because the collision was.
        assert_eq!(next_name(&rows, &shelves, "t", "1"), "1_1");
        // A level with nothing on it needs no counter at all — the namer still
        // answers with one, and the caller that asked is the one that knows a
        // collision happened.
        assert_eq!(next_name(&rows, &shelves, "empty", "1"), "1_1");
        // And a counter does not stack on a counter.
        assert_eq!(next_name(&rows, &shelves, "s", "1_1"), "1_2");
    }

    #[test]
    fn an_arrival_carries_its_own_name_and_its_file() {
        let at = Arrival::import(file("1.pdf"), "s", Some(2));
        assert_eq!(at.name, "1", "the name is the stem: a title is not a file name");
        assert!(at.is_import());
        assert_eq!(at.moving, None);
        assert_eq!(at.index, Some(2));
        assert_eq!(at.file.as_ref().map(|f| f.path.as_str()), Some("/books/1.pdf"));
        let moved = Arrival::moved("b1", "Dune", ALL_SHELF, None);
        assert!(!moved.is_import());
        assert_eq!(moved.moving.as_deref(), Some("b1"));
        assert_eq!(moved.name, "Dune");
    }

    #[test]
    fn a_link_row_is_not_a_book_and_says_so() {
        let rows = [book("b1", "/books/1.pdf"), link("l1", "1", "b1")];
        assert!(rows[0].is_book() && !rows[0].is_link());
        assert!(rows[1].is_link() && !rows[1].is_book());
        assert_eq!(rows[1].book(), None);
        assert_eq!(rows[1].fp(), None, "a pointer has no content identity");
        assert_eq!(rows[1].target(), Some("b1"));
        assert_eq!(rows[0].target(), None);
        assert_eq!(rows[1].display_name(), "1");
        assert_eq!(rows[1].id(), "l1");
        assert_eq!(rows[0].id(), "b1");
    }
}
