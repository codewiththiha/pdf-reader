//! How a shelf is ordered.
//!
//! One comparator over [`Book`], driven by a key and a direction — no
//! secondary key, deliberately. "Then by…" is a second control that doubles
//! the menu and answers a question readers do not ask: within a title, the
//! address breaks the tie, which is stable, deterministic and free.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::book::Book;

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
fn ascending(a: &Book, b: &Book, key: SortKey) -> Ordering {
    let primary = match key {
        // Manual never reaches here: the caller leaves the list alone.
        SortKey::Manual => Ordering::Equal,
        SortKey::Title => natural(&a.title(), &b.title()),
        SortKey::Author => match (a.author(), b.author()) {
            // No author sorts after every author, so a shelf of named books
            // reads as a shelf rather than as a list with blanks in it.
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => natural(&x, &y),
        },
        SortKey::Added => a.added_ms.cmp(&b.added_ms),
        SortKey::LastRead => a.last_read_ms.cmp(&b.last_read_ms),
    };
    primary.then_with(|| a.path().cmp(b.path()))
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

/// Sort a shelf's books in place. [`SortKey::Manual`] leaves the reader's
/// arrangement exactly as it is — which is the point of calling this with
/// whatever the view says rather than branching at every call site.
///
/// `asc` inverts the key's order but never the tie-break, so a descending
/// title sort is still deterministic.
pub fn sort_books(books: &mut [Book], key: SortKey, asc: bool) {
    if key.is_manual() {
        return;
    }
    books.sort_by(|a, b| {
        let ord = ascending(a, b, key);
        if asc {
            ord
        } else {
            ord.reverse()
        }
    });
}

/// The order a shelf renders in: the member ids, resolved to books, sorted.
/// Members naming a book the library no longer has are dropped — a stale
/// membership must not become a hole in the grid.
pub fn ordered(books: &[Book], members: &[String], key: SortKey, asc: bool) -> Vec<Book> {
    let mut out: Vec<Book> = members
        .iter()
        .filter_map(|id| books.iter().find(|b| &b.id == id).cloned())
        .collect();
    sort_books(&mut out, key, asc);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Fingerprint, Origin};
    use reader_core::format::Format;

    fn book(title: &str, author: Option<&str>) -> Book {
        Book {
            id: title.to_string(),
            fp: Fingerprint {
                size: 1,
                mtime_ms: 1,
                head_hash: 1,
            },
            title: Some(title.to_string()),
            author: author.map(str::to_string),
            format: Format::Pdf,
            origin: Origin::Linked {
                src: format!("/books/{title}.pdf"),
            },
            added_ms: 0,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
        }
    }

    fn titles(books: &[Book]) -> Vec<String> {
        books.iter().map(|b| b.title()).collect()
    }

    #[test]
    fn manual_is_the_reader_s_own_order() {
        let mut books = vec![book("Zebra", None), book("Apple", None)];
        sort_books(&mut books, SortKey::Manual, true);
        assert_eq!(titles(&books), vec!["Zebra", "Apple"]);
        assert!(SortKey::Manual.is_manual());
        assert!(!SortKey::Title.is_manual());
    }

    #[test]
    fn titles_sort_ignoring_case_and_both_ways() {
        let mut books = vec![book("dune", None), book("Apple", None), book("child", None)];
        sort_books(&mut books, SortKey::Title, true);
        assert_eq!(titles(&books), vec!["Apple", "child", "dune"]);
        sort_books(&mut books, SortKey::Title, false);
        assert_eq!(titles(&books), vec!["dune", "child", "Apple"]);
    }

    #[test]
    fn volume_numbers_sort_as_numbers() {
        let mut books = vec![
            book("Volume 10", None),
            book("Volume 2", None),
            book("Volume 1", None),
        ];
        sort_books(&mut books, SortKey::Title, true);
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
        let mut books = vec![book("Dune Messiah", None), book("Dune", None)];
        sort_books(&mut books, SortKey::Title, true);
        assert_eq!(titles(&books), vec!["Dune", "Dune Messiah"]);
    }

    #[test]
    fn books_with_no_author_sort_last() {
        let mut books = vec![
            book("b", None),
            book("a", Some("Zebra")),
            book("c", Some("Apple")),
        ];
        sort_books(&mut books, SortKey::Author, true);
        assert_eq!(titles(&books), vec!["c", "a", "b"]);
        sort_books(&mut books, SortKey::Author, false);
        assert_eq!(titles(&books), vec!["b", "a", "c"]);
    }

    #[test]
    fn the_dates_sort_by_their_own_stamp() {
        let mut books = vec![book("b", None), book("a", None), book("c", None)];
        books[0].added_ms = 20;
        books[1].added_ms = 30;
        books[2].added_ms = 10;
        sort_books(&mut books, SortKey::Added, true);
        assert_eq!(titles(&books), vec!["c", "b", "a"]);
        books[0].last_read_ms = 5;
        books[2].last_read_ms = 9;
        sort_books(&mut books, SortKey::LastRead, false);
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
        let mut books = vec![a, b];
        sort_books(&mut books, SortKey::Title, true);
        assert_eq!(
            books.iter().map(|b| b.path()).collect::<Vec<_>>(),
            vec!["/books/aaa.pdf", "/books/zzz.pdf"]
        );
    }

    #[test]
    fn a_stale_membership_is_dropped_rather_than_rendered_as_a_hole() {
        let books = vec![book("a", None), book("b", None)];
        let members = vec!["b".to_string(), "gone".to_string(), "a".to_string()];
        assert_eq!(titles(&ordered(&books, &members, SortKey::Manual, true)), vec!["b", "a"]);
        assert_eq!(titles(&ordered(&books, &members, SortKey::Title, true)), vec!["a", "b"]);
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
