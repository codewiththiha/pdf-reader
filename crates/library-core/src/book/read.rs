//! Where the reader left off, and how a read is written down.
//!
//! A book is the library's row and the reader's resume point at once, so a read
//! is an UPDATE to a row rather than an insert into a second list — which is what
//! ended the drift the old "recent books" record had: import a folder and a book
//! had an address with no resume point, open it and the resume point had no shelf.

use super::{Book, Fingerprint, Origin, Row, find_by_id};

/// Where the reader is in a book: the resume page, the page count, and (for a
/// reflowable document read as one continuous stream) the fraction along it.
///
/// One value rather than three loose arguments, because the three always
/// travel together — the reader reports them as a set on every open and on
/// every progress save — and a call site that could pass one without the
/// others is a call site that could desynchronise a book from itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadPoint {
    pub page: u32,
    pub num_pages: u32,
    /// `None` unless a stream was live; a page is the whole truth otherwise.
    pub fraction: Option<f64>,
}

impl ReadPoint {
    /// A reader that has just opened a document and got no further than the
    /// first page of an unknown length.
    pub fn fresh() -> Self {
        Self {
            page: 1,
            num_pages: 0,
            fraction: None,
        }
    }

    /// The point with an impossible fraction dropped and the page clamped to
    /// the first. What [`record_read`] writes before the row is touched, so no
    /// writer can hand the library a resume point it has to second-guess
    /// later.
    pub fn settled(self) -> Self {
        Self {
            page: self.page.max(1),
            num_pages: self.num_pages,
            fraction: self.fraction.filter(|f| (0.0..=1.0).contains(f)),
        }
    }
}

/// Record a read: every book at `path` moves to `now_ms` with the resume point
/// the reader just reached, or the library gains a linked book when the reader
/// opened something it did not know.
///
/// EVERY book at the path, because a shelf can hold two rows of one file — a
/// duplicate the reader asked to keep (see [`duplicate_title`]) — and the
/// reading position is a fact about the FILE, not about the row: both copies
/// of "dune.pdf" resume where the reader left off in it. One row per address
/// was the old rule and it is still the common case; this is what makes the
/// uncommon one honest.
///
/// Which rows that is, is [`rows_for_read`]'s answer and not this function's:
/// every shared row at the address, and a book of its own
/// ([`Book::independent`]) only when the reader named it
/// ([`record_read_row`]).
///
/// Returns the new book when one was created, so the caller can put it at the
/// front of the "All" order — an existing book keeps the position the reader
/// gave it, because a library is arranged, not a recents list.
///
/// `point` is the reader's own truth, written through settled. `title` and
/// `author` only ever fill a gap, so a name the document supplied at first open
/// survives every later resume — a scan cannot know either, and a second open of
/// the same file must not blank what the first one learned. A duplicate's own
/// name is a value, not a gap, so a shared read never overwrites it.
///
/// A NEW book gets a placeholder fingerprint and the pending mark, because the
/// reader has proved the file opens and nothing more: it has measured no size,
/// no stamp and no bytes. [`apply_check`] is the only thing that writes a
/// measured fingerprint, and it runs on the path check that follows.
pub fn record_read(
    rows: &mut Vec<Row>,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, None, path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref()).map(str::to_string);
    let author = crate::text::non_blank(author.as_deref()).map(str::to_string);
    let book = Book {
        title,
        author,
        last_read_ms: now_ms,
        page: point.page,
        num_pages: point.num_pages,
        fraction: point.fraction,
        fp_pending: true,
        ..Book::new(
            crate::id::next_id(now_ms),
            Fingerprint::placeholder(path),
            reader_core::format::format_of(path),
            Origin::Linked {
                src: path.to_string(),
            },
            now_ms,
        )
    };
    rows.insert(0, Row::Book(book.clone()));
    Some(book)
}

/// Write one read to every row [`rows_for_read`] named. Answers whether it
/// wrote anything, which is the caller's whole question: a read that found its
/// rows is done, and one that found none owes the library a book.
fn write_read(
    rows: &mut [Row],
    at: &[usize],
    title: &Option<String>,
    author: &Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> bool {
    if at.is_empty() {
        return false;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref());
    let author = crate::text::non_blank(author.as_deref());
    for i in at {
        // A link is never in the list `rows_for_read` answers with, and the
        // `else` is the guard that keeps that a property of this function
        // rather than of its caller.
        let Some(book) = rows.get_mut(*i).and_then(Row::as_book_mut) else {
            continue;
        };
        book.page = point.page;
        book.num_pages = point.num_pages;
        book.fraction = point.fraction;
        book.last_read_ms = now_ms;
        book.missing = false;
        if crate::text::non_blank(book.title.as_deref()).is_none() {
            if let Some(t) = title {
                book.title = Some(t.to_string());
            }
        }
        if book.author.is_none() {
            book.author = author.map(str::to_string);
        }
    }
    true
}

/// The rows a reading position belongs to.
///
/// The row the reader NAMED, when that row is a book of its own — an
/// independent row's resume point is the reader's answer for that book and no
/// other — and every shared row at the address otherwise, because a position
/// is a fact about the FILE and two rows of one file resume together.
///
/// A named row that is shared answers with its twins rather than with itself,
/// which is what makes naming a row safe: the id says WHICH book the reader
/// opened, and a shared book's position is not its own to keep. An address
/// whose rows are all independent falls back to all of them when no row was
/// named — an open that cannot say which book it was treats the address as one
/// book, the rule it has always followed, and the alternative is a second row
/// minted for a file the library already holds.
///
/// Indices rather than references, so a caller can hold the answer across the
/// mutable write it is about to make, and so the three writers of a resume
/// point — the open's record ([`record_read`]), the progress debounce and the
/// close's flush — read one rule rather than three.
pub fn rows_for_read(rows: &[Row], book_id: Option<&str>, path: &str) -> Vec<usize> {
    let named = book_id
        .and_then(|id| find_by_id(rows, id))
        .filter(|b| b.path() == path);
    if let Some(book) = named
        && book.independent
    {
        let id = book.id.clone();
        return rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id() == id)
            .map(|(i, _)| i)
            .collect();
    }
    let shared: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path && !b.independent))
        .map(|(i, _)| i)
        .collect();
    if !shared.is_empty() {
        return shared;
    }
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path))
        .map(|(i, _)| i)
        .collect()
}

/// [`record_read`] for an open that knows WHICH row the reader meant: a card,
/// a list row or the context menu's Open all carry the book's id, and an id is
/// the only thing that can tell two rows of one address apart.
///
/// A shared row answers exactly as [`record_read`] does — every shared row at
/// the address moves, because that is what sharing means — and an independent
/// one moves alone, which is the whole of what makes it a book of its own. A
/// row that went while the document was open falls back to the address's own
/// rule rather than dropping the read on the floor: the reader read something,
/// and `path` is what they read.
///
/// Returns the new book when one was created, for the same reason
/// [`record_read`] does.
pub fn record_read_row(
    rows: &mut Vec<Row>,
    book_id: &str,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, Some(book_id), path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    record_read(rows, path, title, author, point, now_ms)
}
