//! The titlebar search: which books a query keeps.
//!
//! Client-side and over three fields, because that is all a library is: the
//! name the document gave (or the file stem, when it gave none), the author,
//! and the address. Nothing here is indexed — a few thousand string comparisons
//! per keystroke is well inside a frame, and an index would be a second thing
//! to keep in step with the list it describes.

use crate::book::Book;

/// True when the query is worth filtering on. A blank bar means "show
/// everything", and a bar of spaces means the same thing — the distinction
/// matters because an empty query must not hide the shelf.
pub fn is_active(query: &str) -> bool {
    !query.trim().is_empty()
}

/// Whether a book survives `query`.
///
/// Every whitespace-separated term has to be found somewhere in the book, in
/// any of the three fields and in any order — so "herbert dune" and "dune
/// herbert" answer the same, and a title with a subtitle is reachable by
/// either half. Comparison is case-insensitive; a library search that cared
/// about case would be a grep, not a search.
pub fn matches(book: &Book, query: &str) -> bool {
    let hay = haystack(book);
    query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .all(|term| hay.contains(&term))
}

/// The three searchable fields, lower-cased and space-joined. Built per call
/// rather than cached: the alternative is a second copy of every title in the
/// library that has to be invalidated on every rename, and the join is three
/// short allocations on a keystroke.
fn haystack(book: &Book) -> String {
    let mut out = book.title().to_lowercase();
    if let Some(author) = book.author() {
        out.push(' ');
        out.push_str(&author.to_lowercase());
    }
    out.push(' ');
    out.push_str(&book.path().to_lowercase());
    out
}

/// The books a query keeps, in the order they were given. The shelf's own sort
/// runs before this, so filtering never re-orders anything.
pub fn filter(books: &[Book], query: &str) -> Vec<Book> {
    if !is_active(query) {
        return books.to_vec();
    }
    books
        .iter()
        .filter(|b| matches(b, query))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Fingerprint, Origin};
    use reader_core::format::Format;

    fn book(title: &str, author: Option<&str>, path: &str) -> Book {
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
                src: path.to_string(),
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

    #[test]
    fn a_blank_bar_filters_nothing() {
        assert!(!is_active(""));
        assert!(!is_active("   "));
        assert!(is_active("d"));
        let books = vec![book("Dune", None, "/books/dune.pdf")];
        assert_eq!(filter(&books, "").len(), 1);
        assert_eq!(filter(&books, "  ").len(), 1);
    }

    #[test]
    fn a_title_is_found_whatever_its_case() {
        let b = book("Dune Messiah", Some("Frank Herbert"), "/books/dune.pdf");
        assert!(matches(&b, "dune"));
        assert!(matches(&b, "DUNE"));
        assert!(matches(&b, "messiah"));
        assert!(!matches(&b, "foundation"));
    }

    #[test]
    fn every_term_has_to_be_found_but_the_order_does_not_matter() {
        let b = book("Dune", Some("Frank Herbert"), "/books/dune.pdf");
        assert!(matches(&b, "herbert dune"));
        assert!(matches(&b, "dune herbert"));
        assert!(matches(&b, "frank  dune"));
        assert!(!matches(&b, "dune asimov"));
    }

    #[test]
    fn the_address_is_searchable_too() {
        // A book whose document carries no title is still reachable by the
        // folder the reader filed it in.
        let b = book("Untitled scan", None, "/Books/Scifi/1984-report.pdf");
        assert!(matches(&b, "scifi"));
        assert!(matches(&b, "1984"));
        assert!(matches(&b, "report"));
    }

    #[test]
    fn a_titleless_book_is_searchable_by_its_stem() {
        let mut b = book("ignored", None, "/books/Foundation.pdf");
        b.title = None;
        assert!(matches(&b, "foundation"), "the stem is the title when there is none");
    }

    #[test]
    fn filtering_keeps_the_order_it_was_given() {
        let books = vec![
            book("Zebra", None, "/books/z.pdf"),
            book("Apple", None, "/books/ap.pdf"),
            book("Apricot", None, "/books/apc.pdf"),
        ];
        let kept: Vec<String> = filter(&books, "ap").iter().map(|b| b.title()).collect();
        assert_eq!(kept, vec!["Apple", "Apricot"], "the shelf's own sort ran first");
        assert!(filter(&books, "q").is_empty());
    }
}
