//! How a shelf is ordered.
//!
//! One comparator over [`Row`], driven by a key and a direction — no
//! secondary key, deliberately. "Then by…" is a second control that doubles
//! the menu and answers a question readers do not ask: within a title, the
//! address breaks the tie, which is stable, deterministic and free.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::book::{Book, Row};

/// What a shelf is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    /// The order the reader arranged — the persisted book list as it stands.
    /// The default, and the one a drag writes to.
    #[default]
    Manual,
    Title,
    Author,
    Added,
    LastRead,
}

impl SortKey {
    /// The label the view menu shows.
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Manual => "Manual",
            SortKey::Title => "Title",
            SortKey::Author => "Author",
            SortKey::Added => "Date added",
            SortKey::LastRead => "Last read",
        }
    }

    /// True when the key re-orders the list rather than leaving the reader's
    /// arrangement alone. What disables a drag: dropping a book at an index of
    /// a list that is sorted by title would be undone by the next render.
    pub fn is_manual(self) -> bool {
        matches!(self, SortKey::Manual)
    }
}

/// The comparison one key makes, ascending. Ties break on the address, so the
/// order is total and stable whatever the list started as.
///
/// A link is a row like any other here and has an answer for every key: its
/// name sorts as a title, it has no author so it sorts after every author, it
/// was added when it was made, it has never been read, and its tie-break is
/// its own id because it has no address of its own. A shelf sorted by title
/// with a pointer in it reads as one shelf rather than as a shelf with a row
/// that jumped to an end.
fn ascending(a: &Row, b: &Row, key: SortKey) -> Ordering {
    let primary = match key {
        // Manual never reaches here: the caller leaves the list alone.
        SortKey::Manual => Ordering::Equal,
        SortKey::Title => natural(&a.display_name(), &b.display_name()),
        SortKey::Author => match (author_of(a), author_of(b)) {
            // No author sorts after every author, so a shelf of named books
            // reads as a shelf rather than as a list with blanks in it.
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => natural(&x, &y),
        },
        SortKey::Added => a.added_ms().cmp(&b.added_ms()),
        SortKey::LastRead => last_read_of(a).cmp(&last_read_of(b)),
    };
    primary.then_with(|| tiebreak(a).cmp(tiebreak(b)))
}

/// The author a row can name: a book's own, and nothing for a link.
fn author_of(row: &Row) -> Option<String> {
    row.book().and_then(Book::author)
}

/// When a row was last read. A link has never been read, which sorts it with
/// the books nobody has opened — the honest end of a "Last read" shelf.
fn last_read_of(row: &Row) -> u64 {
    row.book().map_or(0, |b| b.last_read_ms)
}

/// The tie-break that makes the order total: a book's address, and a link's own
/// id, which is the only thing about it that is stable and unique.
fn tiebreak(row: &Row) -> &str {
    match row {
        Row::Book(b) => b.path(),
        Row::Link { id, .. } => id,
    }
}

/// Case-insensitive compare that also puts "chapter 2" before "chapter 10".
///
/// Digit runs are compared by value rather than by first differing character,
/// which is the only part of this that is not `eq_ignore_ascii_case`: a
/// library is full of volume numbers, and "Volume 10" sorting between
/// "Volume 1" and "Volume 2" is the kind of wrong a reader notices once and
/// never forgives. Non-ASCII keeps its byte order — folding Unicode properly
/// is a collation library, and a shelf of PDFs does not need one.
fn natural(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let si = i;
            let sj = j;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            // Leading zeros are not significant: "01" and "1" are the same
            // number, and comparing the stripped runs keeps them adjacent.
            let na = strip_zeros(&a[si..i]);
            let nb = strip_zeros(&b[sj..j]);
            let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let (xl, yl) = (x.to_ascii_lowercase(), y.to_ascii_lowercase());
            if xl != yl {
                return xl.cmp(&yl);
            }
            i += 1;
            j += 1;
        }
    }
    // One name is a prefix of the other: the shorter one comes first.
    (a.len() - i).cmp(&(b.len() - j))
}

fn strip_zeros(digits: &[u8]) -> &[u8] {
    let first = digits.iter().position(|d| *d != b'0').unwrap_or(digits.len());
    &digits[first.min(digits.len().saturating_sub(1))..]
}

/// Sort a level's rows in place. [`SortKey::Manual`] leaves the reader's
/// arrangement exactly as it is — which is the point of calling this with
/// whatever the view says rather than branching at every call site.
///
/// `asc` inverts the key's order but never the tie-break, so a descending
/// title sort is still deterministic.
pub fn sort_rows(rows: &mut [Row], key: SortKey, asc: bool) {
    if key.is_manual() {
        return;
    }
    rows.sort_by(|a, b| {
        let ord = ascending(a, b, key);
        if asc {
            ord
        } else {
            ord.reverse()
        }
    });
}

/// The order a shelf renders in: the member ids, resolved to rows, sorted.
/// Members naming a row the library no longer has are dropped — a stale
/// membership must not become a hole in the grid — and a member naming a LINK
/// resolves to the link, which is a row the shelf renders like any other.
///
/// The resolution is one pass over the list into a map and one lookup per
/// member, rather than a walk of the library per member: a level renders on
/// every change to the books, and a shelf of hundreds inside a library of
/// thousands is exactly where a quadratic walk becomes a dropped frame.
pub fn ordered(rows: &[Row], members: &[String], key: SortKey, asc: bool) -> Vec<Row> {
    let index = crate::book::index_by_id(rows);
    let mut out: Vec<Row> = members
        .iter()
        .filter_map(|id| index.get(id.as_str()).map(|row| (*row).clone()))
        .collect();
    sort_rows(&mut out, key, asc);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Origin, book_rows};

    fn book(title: &str, author: Option<&str>) -> Book {
        Book {
            title: Some(title.to_string()),
            author: author.map(str::to_string),
            ..crate::testkit::book(title)
        }
    }

    /// The rows a list of books makes: a shelf of books and no links, which is
    /// what every ordering rule here is about.
    fn rows(books: impl IntoIterator<Item = Book>) -> Vec<Row> {
        books.into_iter().map(Row::Book).collect()
    }

    fn at_mut(rows: &mut [Row], i: usize) -> &mut Book {
        rows[i].as_book_mut().expect("a book row")
    }

    fn titles(rows: &[Row]) -> Vec<String> {
        rows.iter().map(Row::display_name).collect()
    }

    #[test]
    fn manual_is_the_reader_s_own_order() {
        let mut books = rows([book("Zebra", None), book("Apple", None)]);
        sort_rows(&mut books, SortKey::Manual, true);
        assert_eq!(titles(&books), vec!["Zebra", "Apple"]);
        assert!(SortKey::Manual.is_manual());
        assert!(!SortKey::Title.is_manual());
    }

    #[test]
    fn titles_sort_ignoring_case_and_both_ways() {
        let mut books = rows([book("dune", None), book("Apple", None), book("child", None)]);
        sort_rows(&mut books, SortKey::Title, true);
        assert_eq!(titles(&books), vec!["Apple", "child", "dune"]);
        sort_rows(&mut books, SortKey::Title, false);
        assert_eq!(titles(&books), vec!["dune", "child", "Apple"]);
    }

    #[test]
    fn volume_numbers_sort_as_numbers() {
        let mut books = rows([
            book("Volume 10", None),
            book("Volume 2", None),
            book("Volume 1", None),
        ]);
        sort_rows(&mut books, SortKey::Title, true);
        assert_eq!(titles(&books), vec!["Volume 1", "Volume 2", "Volume 10"]);
    }

    #[test]
    fn leading_zeros_do_not_make_a_separate_run() {
        assert_eq!(natural("ch 01", "ch 1"), Ordering::Equal);
        assert_eq!(natural("01", "1"), Ordering::Equal);
        assert!(natural("a02", "a10").is_lt());
    }

    #[test]
    fn a_prefix_comes_before_the_longer_name() {
        let mut books = rows([book("Dune Messiah", None), book("Dune", None)]);
        sort_rows(&mut books, SortKey::Title, true);
        assert_eq!(titles(&books), vec!["Dune", "Dune Messiah"]);
    }

    #[test]
    fn books_with_no_author_sort_last() {
        let mut books = rows([
            book("b", None),
            book("a", Some("Zebra")),
            book("c", Some("Apple")),
        ]);
        sort_rows(&mut books, SortKey::Author, true);
        assert_eq!(titles(&books), vec!["c", "a", "b"]);
        sort_rows(&mut books, SortKey::Author, false);
        assert_eq!(titles(&books), vec!["b", "a", "c"]);
    }

    #[test]
    fn the_dates_sort_by_their_own_stamp() {
        let mut books = rows([book("b", None), book("a", None), book("c", None)]);
        at_mut(&mut books, 0).added_ms = 20;
        at_mut(&mut books, 1).added_ms = 30;
        at_mut(&mut books, 2).added_ms = 10;
        sort_rows(&mut books, SortKey::Added, true);
        assert_eq!(titles(&books), vec!["c", "b", "a"]);
        at_mut(&mut books, 0).last_read_ms = 5;
        at_mut(&mut books, 2).last_read_ms = 9;
        sort_rows(&mut books, SortKey::LastRead, false);
        assert_eq!(titles(&books), vec!["a", "c", "b"]);
    }

    #[test]
    fn equal_keys_fall_back_to_the_address_so_the_order_is_total() {
        let mut a = book("Same", None);
        let mut b = book("Same", None);
        a.origin = Origin::Linked { src: "/books/zzz.pdf".into() };
        b.origin = Origin::Linked { src: "/books/aaa.pdf".into() };
        a.id = "one".into();
        b.id = "two".into();
        let mut books = rows([a, b]);
        sort_rows(&mut books, SortKey::Title, true);
        assert_eq!(
            book_rows(&books).map(|b| b.path()).collect::<Vec<_>>(),
            vec!["/books/aaa.pdf", "/books/zzz.pdf"]
        );
    }

    #[test]
    fn a_stale_membership_is_dropped_rather_than_rendered_as_a_hole() {
        let books = rows([book("a", None), book("b", None)]);
        let members = vec!["b".to_string(), "gone".to_string(), "a".to_string()];
        assert_eq!(titles(&ordered(&books, &members, SortKey::Manual, true)), vec!["b", "a"]);
        assert_eq!(titles(&ordered(&books, &members, SortKey::Title, true)), vec!["a", "b"]);
    }

    #[test]
    fn a_link_sorts_as_the_row_it_shows() {
        // A pointer has a name, no author, a stamp of its own and no reading
        // at all — which is an answer for every key rather than a row the sort
        // has to skip.
        let mut list = rows([book("Dune", Some("Frank Herbert")), book("Apple", None)]);
        list.push(Row::link("l1".into(), "Child".into(), "b1".into(), 5));
        sort_rows(&mut list, SortKey::Title, true);
        assert_eq!(titles(&list), vec!["Apple", "Child", "Dune"]);
        sort_rows(&mut list, SortKey::Author, true);
        assert_eq!(
            titles(&list),
            vec!["Dune", "Apple", "Child"],
            "no author sorts after every author, and a link has none"
        );
        sort_rows(&mut list, SortKey::LastRead, true);
        assert_eq!(titles(&list).last().map(String::as_str), Some("Child"));
    }

    #[test]
    fn every_key_has_a_label_the_menu_can_show() {
        for key in [
            SortKey::Manual,
            SortKey::Title,
            SortKey::Author,
            SortKey::Added,
            SortKey::LastRead,
        ] {
            assert!(!key.label().is_empty());
        }
        // The labels are storage-adjacent copy, so they survive a round trip
        // of the value rather than of the string.
        let json = serde_json::to_string(&SortKey::LastRead).unwrap();
        assert_eq!(json, "\"lastRead\"");
        let back: SortKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SortKey::LastRead);
    }
}
