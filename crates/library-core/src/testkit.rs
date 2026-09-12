//! Fixture builders for the library's host tests — one spelling of "a book",
//! "a row", "a link" and "a shelf" for every test module that used to build
//! its own.
//!
//! Ten test modules across the crate and the app each hand-rolled a `Book`
//! literal of fifteen fields to vary one or two of them, and the ten literals
//! drifted: one spelled the fingerprint `{1,1,1}`, another `{size,len}`, and
//! a reader comparing two suites compared fixtures rather than rules. The
//! builders here are the shared spine; a test whose rule reads a field the
//! spine blanks overrides THAT field with a struct update —
//! `Book { page: 12, ..testkit::book("b1") }` — so the variation is the only
//! thing on the page.
//!
//! Gated behind `test-util` (or the crate's own `cfg(test)`) so no fixture
//! ever reaches a release bundle: the app crate's tests opt in through its
//! dev-dependency on this crate, and nothing else can name the feature.
//!
//! Values are chosen to be inert: a Markdown row keeps the app's cover queue
//! from starting the wasm render chain in a host test, and `/books/{id}` is
//! an address no assertion depends on beyond "distinct per id".

use crate::book::{Book, Fingerprint, Origin, Row};
use crate::scan::FoundFile;
use crate::shelf::{Shelf, ShelfKind};
use reader_core::format::Format;

/// The neutral fingerprint: every field `1`, so two fixtures of one id match
/// and a test that cares about a measurement says so by varying one.
pub fn fingerprint() -> Fingerprint {
    Fingerprint {
        size: 1,
        mtime_ms: 1,
        head_hash: 1,
    }
}

/// A fingerprint whose three fields all read `n` — the "file number n" of a
/// ledger test, where the value only has to be distinct per file.
pub fn fp_n(n: u32) -> Fingerprint {
    Fingerprint {
        size: u64::from(n),
        mtime_ms: u64::from(n),
        head_hash: n,
    }
}

/// A minimal linked book: id `id`, address `/books/{id}.pdf`, format PDF,
/// joined at `0`, page 1, nothing read, nothing missing, nothing pending.
pub fn book(id: &str) -> Book {
    Book::new(
        id.to_string(),
        fingerprint(),
        Format::Pdf,
        Origin::Linked {
            src: format!("/books/{id}.pdf"),
        },
        0,
    )
}

/// [`book`] as a row.
pub fn row(id: &str) -> Row {
    Row::Book(book(id))
}

/// A linked Markdown book at `/books/{id}.md` — the fixture an app-side test
/// lands through a placement, where a PDF would ask the cover queue for a
/// render no host test can serve.
pub fn markdown_book(id: &str) -> Book {
    Book::new(
        id.to_string(),
        fingerprint(),
        Format::Markdown,
        Origin::Linked {
            src: format!("/books/{id}.md"),
        },
        0,
    )
}

/// [`markdown_book`] as a row.
pub fn markdown_row(id: &str) -> Row {
    Row::Book(markdown_book(id))
}

/// A book whose display name is `title` — what a collision, a sort and a
/// receipt read.
pub fn titled_book(id: &str, title: &str) -> Book {
    Book {
        title: Some(title.to_string()),
        ..book(id)
    }
}

/// [`titled_book`] as a row.
pub fn titled_row(id: &str, title: &str) -> Row {
    Row::Book(titled_book(id, title))
}

/// A book row at `path` — the id and the address differ, which is what a
/// relink, a store copy and a twin test need.
pub fn book_at(id: &str, path: &str) -> Book {
    Book {
        origin: Origin::Linked {
            src: path.to_string(),
        },
        ..book(id)
    }
}

/// [`book_at`] as a row.
pub fn row_at(id: &str, path: &str) -> Row {
    Row::Book(book_at(id, path))
}

/// A pointer row: a link called `name` at `target`, made at `0`.
pub fn link(id: &str, name: &str, target: &str) -> Row {
    Row::link(id.to_string(), name.to_string(), target.to_string(), 0)
}

/// A virtual shelf: the reader's own, holding `members`, hanging under
/// `parent` (`None` is the root level).
pub fn shelf(id: &str, name: &str, members: &[&str], parent: Option<&str>) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: name.to_string(),
        kind: ShelfKind::Virtual,
        books: members.iter().map(|m| m.to_string()).collect(),
        parent: parent.map(str::to_string),
        manual_parent: false,
    }
}

/// A shelf named by its id — the common case, where the name is not what the
/// test is about.
pub fn plain_shelf(id: &str, members: &[&str]) -> Shelf {
    shelf(id, id, members, None)
}

/// A folder shelf: one rung of a watched folder's tree, at `rel` (`None` is
/// the folder's root rung).
pub fn folder_shelf(
    id: &str,
    name: &str,
    folder_id: &str,
    rel: Option<&str>,
    members: &[&str],
    parent: Option<&str>,
) -> Shelf {
    Shelf {
        kind: ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: rel.map(str::to_string),
        },
        ..shelf(id, name, members, parent)
    }
}

/// A found Markdown file numbered `n`: the walk's row for "file number n",
/// whose `rel` is its own name — a test that needs a subfolder `rel` says so
/// with a struct update.
pub fn found_md(path: &str, n: u32) -> FoundFile {
    FoundFile {
        rel: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        ext: "md".to_string(),
        size: u64::from(n),
        fp: fp_n(n),
    }
}
