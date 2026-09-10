//! One book: its identity, where its bytes live, and where the reader left
//! off.
//!
//! A book is the library's row and the reader's resume point at once — the
//! page, the page count and the reflowable stream fraction used to live on a
//! separate "recent books" record, which meant the same path was written down
//! twice and the two copies drifted (import a folder and a book had an
//! address with no resume point; open it and the resume point had no shelf).
//! One record ends that: [`Book`] carries both, and opening a document is an
//! update to the book rather than an insert into a second list.

use serde::{Deserialize, Serialize};

use reader_core::filename::file_stem_from_path;
use reader_core::format::Format;

/// Storage guard on the library's size. This is NOT a "recent books" cap — a
/// library is a collection, and trimming it to twenty would drop books the
/// user imported on purpose. It is the guard that keeps the persisted blob
/// (`blob::LibraryBlob`, one localStorage key) inside a browser's quota:
/// two thousand rows of paths and titles is a few hundred kilobytes. Past it
/// the least-recently-read books go, which is the only eviction order that
/// cannot drop something the reader is in the middle of.
pub const BOOKS_CAP: usize = 2000;

/// A book's content identity: what a rescan compares a file on disk against.
///
/// `size` + `mtime_ms` is what a move preserves, so a file dragged to another
/// folder inside a watched tree still resolves to the book it already is —
/// that is the [`ledger::ScanAction::Relink`](crate::ledger::ScanAction) case.
/// `head_hash` is the cheap disambiguator for the collision that matters (two
/// different books of exactly the same length, touched in the same
/// millisecond): FNV-1a over the first 8 KiB, see [`crate::hash`].
///
/// Deliberately NOT a whole-file hash. A library is scanned on every window
/// focus, and reading 2 GB of PDFs to answer "is this the book I placed last
/// week" is not a trade worth making; the first 8 KiB of a PDF is its header
/// and the start of its first object, which is where two books differ most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    /// Modification time in milliseconds since the Unix epoch. Files whose
    /// stamp predates the epoch (or a filesystem that reports none) land on 0
    /// rather than on a wrapped value — [`crate::hash`] owns that conversion.
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl Fingerprint {
    /// The fingerprint of a file's first bytes and metadata. One call site per
    /// side of the wire: the shell's folder walk and the shell's copy, so a
    /// linked book and its stored twin agree.
    pub fn of(size: u64, mtime_ms: u64, head: &[u8]) -> Self {
        Self {
            size,
            mtime_ms,
            head_hash: crate::hash::head_hash(head),
        }
    }

    /// A stand-in for a book the library knows by address only: a row migrated
    /// from the `v1` schema, which stored a path and a resume point and
    /// measured nothing. Derived from the address so two different books can
    /// never share one, and stamped with `mtime_ms == 0` so it reads as what
    /// it is — not a measurement.
    ///
    /// Replaced by [`Fingerprint::of`] the first time the file is checked or
    /// read; [`Book::fp_pending`] says which books are still waiting for that.
    pub fn placeholder(path: &str) -> Self {
        let bytes = path.as_bytes();
        Self {
            size: bytes.len() as u64,
            mtime_ms: 0,
            head_hash: crate::hash::head_hash(bytes),
        }
    }
}

/// How the app holds a book's bytes.
///
/// The library was path-addressed before this existed — `open_path` hands the
/// raw path to the pdf.js bridge and the reflowable formats read it through
/// the shell's `read_file_text`, so nothing was ever copied into the app.
/// [`Origin::Linked`] is that mode, and it stays the default. [`Origin::Stored`]
/// is the new one: a copy inside the app's own store, so a book survives the
/// folder it came from being renamed, moved or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// Read in place: the book IS this path. The app never moves, renames or
    /// deletes it, and a vanished path is a [`Book::missing`] book with a
    /// Relink affordance — never a silent removal.
    Linked { src: String },
    /// Copied into the app's store. `src` is provenance: what the copy was
    /// made from, kept so a Relink can offer to copy again from a source that
    /// is still there. `None` for a book whose source was already gone.
    Stored { src: Option<String>, store: String },
}

impl Origin {
    /// The path the reader opens: the source for a linked book, the store copy
    /// for a stored one. This is the address every other layer speaks — the
    /// cover cache keys on it, the resume point is looked up by it, and the
    /// shell's `read_file_*` gate is asked about it.
    pub fn path(&self) -> &str {
        match self {
            Origin::Linked { src } => src,
            Origin::Stored { store, .. } => store,
        }
    }

    /// Where the bytes came from, when the app knows. Differs from [`path`]
    /// only for a stored book, and is what a relink-to-source offer reads.
    pub fn source(&self) -> Option<&str> {
        match self {
            Origin::Linked { src } => Some(src),
            Origin::Stored { src, .. } => src.as_deref(),
        }
    }

    /// True when the app owns the bytes and may delete them with the book.
    pub fn is_stored(&self) -> bool {
        matches!(self, Origin::Stored { .. })
    }
}

/// One book on a shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// Stable identity, independent of the address: a book keeps its id
    /// through a relink, a rename and a move between shelves. This is the
    /// drag payload and the shelf member.
    pub id: String,
    /// Content identity — what a rescan matches against. See [`Fingerprint`].
    pub fp: Fingerprint,
    /// Display name captured at open time (a trustworthy `/Title`, else the
    /// file stem). `None` until the book has been opened once, which is why
    /// [`Book::title`] falls back to the stem rather than storing a guess.
    #[serde(default)]
    pub title: Option<String>,
    /// Author, when the document or the import supplied one.
    #[serde(default)]
    pub author: Option<String>,
    pub format: Format,
    pub origin: Origin,
    /// When the book joined the library, in milliseconds since the epoch —
    /// the "Date added" sort's key.
    #[serde(default)]
    pub added_ms: u64,
    /// When the reader last opened it. `0` for a book imported but never read,
    /// which sorts last rather than first.
    #[serde(default)]
    pub last_read_ms: u64,
    /// 1-based page the reader last reached — the resume point.
    #[serde(default = "default_page")]
    pub page: u32,
    /// Total page count, for the "page X of Y" hint. 0 when unknown.
    #[serde(default)]
    pub num_pages: u32,
    /// Fractional position (0..=1) inside the continuous reading of a
    /// reflowable document. Written only while stream mode is live; `None`
    /// everywhere else, where the page above is the whole truth.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// The address no longer resolves. Set by a path check, never by a scan:
    /// a missing book keeps its row and its shelf membership so a Relink can
    /// heal it, and is only dropped when the reader says so.
    #[serde(default)]
    pub missing: bool,
    /// The fingerprint is a placeholder derived from the address, not a
    /// measurement of the file — the mark a migrated book carries until the
    /// first path check replaces it. A rescan must not run while any book
    /// carries one (see [`crate::blob::LibraryBlob::awaiting_check`]): real
    /// fingerprints compared against placeholders match nothing, so a folder
    /// holding books the reader already has would add them all again.
    #[serde(default)]
    pub fp_pending: bool,
}

fn default_page() -> u32 {
    1
}

impl Book {
    /// A book just joined to the library: its identity, its content identity,
    /// its pipeline, the address it is read from, and the moment it joined —
    /// everything else is the blank a reader fills in later.
    ///
    /// One constructor rather than a struct literal per importing path, so a
    /// joined book starts at page 1, with no title of its own and not missing,
    /// in every one of them. The paths that mint a row — a folder walk, a
    /// handful of loose files, a restore and a hand-open ([`record_read`]) —
    /// override only the fields they have an answer for: an open has measured
    /// nothing, so it stamps a [`Fingerprint::placeholder`] and the
    /// [`Book::fp_pending`] mark on top.
    pub fn new(id: String, fp: Fingerprint, format: Format, origin: Origin, added_ms: u64) -> Self {
        Self {
            id,
            fp,
            title: None,
            author: None,
            format,
            origin,
            added_ms,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
        }
    }

    /// The address this book is read from. See [`Origin::path`].
    pub fn path(&self) -> &str {
        self.origin.path()
    }

    /// The name to show: the document's own title, else the file stem, else
    /// the address. Never empty — a card with no name is a card the reader
    /// cannot tell from its neighbour.
    pub fn title(&self) -> String {
        self.title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| file_stem_from_path(self.path()))
            .unwrap_or_else(|| self.path().to_string())
    }

    /// The author line, when there is one to show.
    pub fn author(&self) -> Option<String> {
        self.author.clone().filter(|a| !a.trim().is_empty())
    }

    /// Reading progress as a fraction of the document, when the page count is
    /// known. What the card's bar and the list row's percentage both draw.
    pub fn progress(&self) -> Option<f64> {
        if self.num_pages == 0 {
            return self.fraction.filter(|f| (0.0..=1.0).contains(f));
        }
        Some((self.page.min(self.num_pages) as f64 / self.num_pages as f64).clamp(0.0, 1.0))
    }
}

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
    /// the first. What [`record_read`] writes, and what the catalog's own SQL
    /// layer in the shell applies before an UPDATE, so no writer in either
    /// process can hand the library a resume point it has to second-guess
    /// later.
    pub fn settled(self) -> Self {
        Self {
            page: self.page.max(1),
            num_pages: self.num_pages,
            fraction: self.fraction.filter(|f| (0.0..=1.0).contains(f)),
        }
    }
}

/// The human-readable stem of an address, or the address when it has none. Never
/// empty, because it is the last fallback a search index and a shelf card both
/// rely on.
pub fn stem_of(path: &str) -> String {
    reader_core::filename::file_stem_from_path(path).unwrap_or_else(|| path.to_string())
}

/// The next free duplicate of `base`: `base_1`, `base_2`, and so on — the
/// counter a file manager appends when a second file of one name has to live
/// beside the first, and the name the library's conflict sheet gives a
/// duplicate the reader chose to keep.
///
/// `in_use` is every name the library already shows. The counter starts at the
/// first free number, and a trailing `_N` on `base` is stripped before
/// counting, so duplicating a duplicate steps instead of stacking: "Dune_1"
/// becomes "Dune_2" rather than "Dune_1_1" — the same reading a file manager
/// gives it, where the counter is not part of the name.
///
/// The result is a title the shelf can keep: never empty (a blank `base`
/// falls back to a word), and the trailing counter is exempt from the
/// filename-shaped rule [`sanitize`] applies to document-supplied titles (the
/// exemption is `reader_core::filename::is_usable_title`'s own — a name this
/// function minted survives the load that reads it back).
pub fn duplicate_title(base: &str, in_use: &std::collections::HashSet<String>) -> String {
    let base = base.trim();
    let root = if base.is_empty() { "Book" } else { base };
    let root = match root.rsplit_once('_') {
        Some((stem, counter))
            if !stem.is_empty()
                && !counter.is_empty()
                && counter.chars().all(|c| c.is_ascii_digit()) =>
        {
            stem
        }
        _ => root,
    };
    (1u32..)
        .map(|n| format!("{root}_{n}"))
        .find(|candidate| !in_use.contains(candidate))
        .expect("an unbounded counter always finds a free name")
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
    books: &mut Vec<Book>,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let point = point.settled();
    let title = title.filter(|t| !t.trim().is_empty());
    let author = author.filter(|a| !a.trim().is_empty());
    let mut found = false;
    for book in books.iter_mut().filter(|b| b.path() == path) {
        found = true;
        book.page = point.page;
        book.num_pages = point.num_pages;
        book.fraction = point.fraction;
        book.last_read_ms = now_ms;
        book.missing = false;
        if book.title.as_deref().map(str::trim).unwrap_or("").is_empty() {
            if let Some(t) = title.clone() {
                book.title = Some(t);
            }
        }
        if book.author.is_none() {
            book.author = author.clone();
        }
    }
    if found {
        return None;
    }
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
    books.insert(0, book.clone());
    Some(book)
}

/// Apply one path check to every book at that address, and say which books it
/// touched.
///
/// Every book at the address, for [`record_read`]'s reason: two rows of one
/// file — a duplicate the reader kept — share the address's fate, and a check
/// that healed one and left the other pending would hold every watched
/// folder's rescan off forever.
///
/// Three outcomes, and the difference between them is the whole reason a
/// library survives a moved folder:
///
///   * the address resolved — the fingerprint becomes the measurement, the
///     pending mark clears, and the book is not missing;
///   * the address did not resolve — the book becomes `missing` and keeps its
///     row, its resume point and every shelf it is on, so a relink can heal it
///     and the reader can still see what they lost. The pending mark clears here
///     too: it means "no check has run", not "the check came back good", and a
///     book left pending forever would hold every watched folder's rescan off;
///   * there is no book at that address — nothing to do.
///
/// Returns the ids the check CHANGED, empty in the third case and for a pass
/// over rows nothing moved, so a startup sweep of a healthy library writes no
/// state at all.
pub fn apply_check(books: &mut [Book], check: &crate::wire::PathCheck) -> Vec<String> {
    let measured = check.fingerprint();
    let mut touched = Vec::new();
    for book in books.iter_mut().filter(|b| b.path() == check.path) {
        let changed = match measured {
            Some(fp) => {
                let changed = book.fp != fp || book.missing || book.fp_pending;
                book.fp = fp;
                book.missing = false;
                book.fp_pending = false;
                changed
            }
            None => {
                // Going missing is news; being told twice is not. A book that
                // was still owed its first measurement is news too, because the
                // pending mark is what holds a watched folder's rescan off.
                let changed = !book.missing || book.fp_pending;
                book.missing = true;
                book.fp_pending = false;
                changed
            }
        };
        if changed {
            touched.push(book.id.clone());
        }
    }
    touched
}

/// Add an imported book at the END of the library's order, or return the id of
/// the one already there. Content identity decides, not the address: the same
/// file reached through a second watched folder is the same book (see
/// [`crate::ledger`]).
///
/// The first row wins when a shelf holds duplicates the reader asked to keep —
/// an import that re-finds a file the library already has twice resolves to
/// the row the library lists first, and which shelf the book then lands on is
/// a question the app's conflict sheet asks BEFORE this is reached (a row
/// whose fingerprint is already on the target shelf is a duplicate/replace/
/// merge choice, not an add).
///
/// Appended rather than pushed to the front, because an import arrives in the
/// order the walk produced — depth-first and alphabetical, which is the order the
/// folder itself lists them — and filing 400 books at the front one at a time
/// would hand the reader that order backwards. A book the reader OPENED goes to
/// the front instead, through [`record_read`]: that is a "what was I reading"
/// question, and this is a "what is on the shelf" one.
pub fn add_book(books: &mut Vec<Book>, book: Book) -> String {
    if let Some(existing) = books.iter().find(|b| b.fp == book.fp) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    books.push(book);
    id
}

/// Remove a book by id, returning it. The caller decides what else the removal
/// implies: a folder ledger tombstone ([`crate::ledger::tombstone`]), a shelf
/// membership ([`crate::shelf::forget`]), and the store copy when the app owns
/// the bytes.
pub fn remove_book(books: &mut Vec<Book>, id: &str) -> Option<Book> {
    let at = books.iter().position(|b| b.id == id)?;
    Some(books.remove(at))
}

/// The resume page for an address, if the library knows it.
pub fn find_page(books: &[Book], path: &str) -> Option<u32> {
    books
        .iter()
        .find(|b| b.path() == path)
        .map(|b| b.page.max(1))
}

/// The saved fractional stream position for an address (see
/// [`Book::fraction`]), for a reflowable document opening back into the
/// continuous mode.
pub fn find_fraction(books: &[Book], path: &str) -> Option<f64> {
    books
        .iter()
        .find(|b| b.path() == path)
        .and_then(|b| b.fraction)
        .filter(|f| (0.0..=1.0).contains(f))
}

/// The book an address resolves to. A relink changes the address a book
/// answers to, so this is the one lookup every path-keyed caller should make.
pub fn find_by_path<'a>(books: &'a [Book], path: &str) -> Option<&'a Book> {
    books.iter().find(|b| b.path() == path)
}

/// Make a persisted list internally valid: drop books with no address or no
/// id, dedupe by id (first wins — the reader's own order), clamp the resume
/// point, and trim to [`BOOKS_CAP`] by least-recently-read. Idempotent.
///
/// By ID and not by content identity: two rows are allowed to share one
/// file's fingerprint when the reader asked to keep both — the library's
/// conflict sheet mints such duplicates under [`duplicate_title`] names — and
/// a sanitizer that deduped by fingerprint would silently undo a choice the
/// reader made. A blob carrying two rows with one ID is the corrupt case this
/// still heals.
pub fn sanitize(books: &mut Vec<Book>) {
    let mut seen = std::collections::HashSet::new();
    books.retain(|b| {
        !b.id.trim().is_empty() && !b.path().trim().is_empty() && seen.insert(b.id.clone())
    });
    for b in books.iter_mut() {
        b.page = b.page.max(1);
        b.fraction = b.fraction.filter(|f| (0.0..=1.0).contains(f));
        // A title that is really a filename — the download name a PDF carries
        // in its metadata — is not a title: drop it and let the stem of the
        // address show, which is the name the reader sees in their own file
        // manager. This is also the heal for rows stored before the rule
        // existed, on the load that first knows better.
        if b.title.as_deref().is_some_and(|t| !reader_core::filename::is_usable_title(t)) {
            b.title = None;
        }
    }
    if books.len() <= BOOKS_CAP {
        return;
    }
    // Trim by least-recently-read rather than by position: the tail of the
    // list is the reader's own arrangement, and rearranging is not the same
    // promise as "I have not opened this in a year".
    let mut by_age: Vec<usize> = (0..books.len()).collect();
    by_age.sort_by_key(|&i| books[i].last_read_ms);
    for i in by_age.into_iter().take(books.len() - BOOKS_CAP) {
        books[i].id = String::new();
    }
    books.retain(|b| !b.id.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(size: u64, mtime: u64, head: u32) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    fn linked(id: &str, path: &str) -> Book {
        Book {
            id: id.to_string(),
            fp: fp(10, 1, 7),
            title: None,
            author: None,
            format: Format::Pdf,
            origin: Origin::Linked {
                src: path.to_string(),
            },
            added_ms: 1,
            last_read_ms: 1,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
        }
    }

    #[test]
    fn a_linked_book_is_the_path_it_was_opened_from() {
        let b = linked("a", "/books/dune.pdf");
        assert_eq!(b.path(), "/books/dune.pdf");
        assert_eq!(b.origin.source(), Some("/books/dune.pdf"));
        assert!(!b.origin.is_stored());
    }

    #[test]
    fn a_stored_book_opens_from_the_store_and_remembers_its_source() {
        let b = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/pdf/dune_1a2b3c4d.pdf".into(),
            },
            ..linked("a", "/ignored")
        };
        assert_eq!(b.path(), "/app/Library/pdf/dune_1a2b3c4d.pdf");
        assert_eq!(b.origin.source(), Some("/downloads/dune.pdf"));
        assert!(b.origin.is_stored());
    }

    #[test]
    fn the_title_falls_back_to_the_stem_and_never_to_nothing() {
        let mut b = linked("a", "/books/Dune.pdf");
        assert_eq!(b.title(), "Dune");
        b.title = Some("  ".into());
        assert_eq!(b.title(), "Dune", "a blank title is no title");
        b.origin = Origin::Linked { src: "/".into() };
        assert_eq!(b.title(), "/", "the address is the last resort");
    }

    #[test]
    fn progress_is_a_page_fraction_or_the_stream_fraction() {
        let mut b = linked("a", "/books/dune.pdf");
        assert_eq!(b.progress(), None, "an unknown page count is no progress");
        b.num_pages = 200;
        b.page = 50;
        assert_eq!(b.progress(), Some(0.25));
        b.page = 900;
        assert_eq!(b.progress(), Some(1.0), "past the end clamps");
        // A reflowable stream has no pages: its fraction is the truth.
        let mut s = linked("s", "/notes.md");
        s.fraction = Some(0.4);
        assert_eq!(s.progress(), Some(0.4));
        s.fraction = Some(1.7);
        assert_eq!(s.progress(), None, "an out-of-range fraction is dropped");
    }

    #[test]
    fn an_author_fills_a_gap_and_never_overwrites_one() {
        let mut books = vec![linked("a", "/books/one.pdf")];
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            Some("Frank Herbert".into()),
            ReadPoint::fresh(),
            1,
        );
        assert_eq!(books[0].author.as_deref(), Some("Frank Herbert"));
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            Some("Somebody Else".into()),
            ReadPoint::fresh(),
            2,
        );
        assert_eq!(
            books[0].author.as_deref(),
            Some("Frank Herbert"),
            "the first author the document gave is the one the shelf keeps"
        );
        // A blank author is no author at all.
        let mut books = vec![linked("b", "/books/two.pdf")];
        record_read(
            &mut books,
            "/books/two.pdf",
            None,
            Some("   ".into()),
            ReadPoint::fresh(),
            1,
        );
        assert_eq!(books[0].author, None);
    }

    #[test]
    fn reading_a_known_book_updates_it_in_place() {
        let mut books = vec![linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")];
        let created = record_read(
            &mut books,
            "/books/two.pdf",
            Some("Two".into()),
            None,
            ReadPoint { page: 42, num_pages: 100, fraction: None },
            500,
        );
        assert!(created.is_none(), "an existing book is not created again");
        assert_eq!(books.len(), 2);
        // The order the reader arranged is untouched.
        assert_eq!(books[0].id, "a");
        assert_eq!(books[1].page, 42);
        assert_eq!(books[1].last_read_ms, 500);
        assert_eq!(books[1].title.as_deref(), Some("Two"));
        // A read is not a measurement: the fingerprint is whatever the last
        // path check found, and this book has already been checked.
        assert_eq!(books[1].fp, fp(10, 1, 7));
        assert!(!books[1].fp_pending);
    }

    #[test]
    fn reading_an_unknown_book_creates_a_linked_one_at_the_front() {
        let mut books = vec![linked("a", "/books/one.pdf")];
        let created = record_read(
            &mut books,
            "/books/new.md",
            None,
            None,
            ReadPoint::fresh(),
            600,
        )
        .expect("a new book");
        assert_eq!(books.len(), 2);
        assert_eq!(books[0].id, created.id);
        assert_eq!(created.format, Format::Markdown);
        assert_eq!(created.path(), "/books/new.md");
        assert!(!created.origin.is_stored());
        // The reader proved the file opens and nothing more, so the identity is
        // a placeholder until the path check that follows measures it.
        assert!(created.fp_pending);
        assert_eq!(created.fp, Fingerprint::placeholder("/books/new.md"));
    }

    #[test]
    fn a_title_only_ever_fills_a_gap() {
        let mut books = vec![Book {
            title: Some("Named by the document".into()),
            ..linked("a", "/books/one.pdf")
        }];
        record_read(
            &mut books,
            "/books/one.pdf",
            Some("one".into()),
            None,
            ReadPoint { page: 3, num_pages: 10, fraction: None },
            10,
        );
        assert_eq!(books[0].title.as_deref(), Some("Named by the document"));
    }

    #[test]
    fn a_resume_point_is_settled_before_it_is_written() {
        // The reader hands over whatever the document said; the library is the
        // thing that has to stay valid, so the clamping happens on the way in
        // rather than at every read of the row.
        let mut books = vec![linked("a", "/books/one.pdf")];
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            None,
            ReadPoint { page: 0, num_pages: 10, fraction: Some(1.4) },
            1,
        );
        assert_eq!(books[0].page, 1);
        assert_eq!(books[0].fraction, None);
        assert_eq!(ReadPoint::fresh(), ReadPoint { page: 1, num_pages: 0, fraction: None });
    }

    #[test]
    fn a_path_check_is_what_measures_a_book() {
        let mut books = vec![Book {
            fp_pending: true,
            ..linked("a", "/books/one.pdf")
        }];
        let touched = apply_check(
            &mut books,
            &check("/books/one.pdf", true, 20, 2, 8),
        );
        assert_eq!(touched, vec!["a".to_string()]);
        assert_eq!(books[0].fp, fp(20, 2, 8));
        assert!(!books[0].fp_pending, "the measurement replaces the placeholder");
        // A second pass over an unchanged file changes nothing, so a startup
        // check of a healthy library writes no state at all.
        assert!(apply_check(&mut books, &check("/books/one.pdf", true, 20, 2, 8)).is_empty());
    }

    #[test]
    fn a_path_that_does_not_resolve_marks_the_book_missing_and_keeps_it() {
        let mut books = vec![Book {
            page: 42,
            num_pages: 100,
            ..linked("a", "/books/one.pdf")
        }];
        assert_eq!(
            apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)),
            vec!["a".to_string()]
        );
        assert!(books[0].missing);
        assert!(!books[0].fp_pending, "a check that ran is not a check still owed");
        assert_eq!(books.len(), 1, "a missing book is not a removed one");
        assert_eq!(books[0].page, 42, "the resume point survives the address dying");
        assert_eq!(books[0].fp, fp(10, 1, 7), "and so does the last known identity");
        // The second pass is not news.
        assert!(apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)).is_empty());
    }

    #[test]
    fn a_check_for_an_address_the_library_does_not_hold_does_nothing() {
        let mut books = vec![linked("a", "/books/one.pdf")];
        assert!(apply_check(&mut books, &check("/books/other.pdf", true, 1, 1, 1)).is_empty());
        assert!(!books[0].missing);
    }

    #[test]
    fn two_rows_of_one_file_share_its_reading_truth() {
        // A duplicate the reader chose to keep is a second ROW, not a second
        // file: the address is read, checked and resumed as one, and a heal
        // that reached only the first row would leave its twin holding a
        // watched folder's rescan off forever.
        let mut books = vec![
            Book {
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
            Book {
                id: "b".into(),
                title: Some("dune_1".into()),
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
        ];
        assert!(record_read(
            &mut books,
            "/books/dune.pdf",
            Some("Dune".into()),
            None,
            ReadPoint { page: 90, num_pages: 400, fraction: None },
            700,
        )
        .is_none());
        for book in &books {
            assert_eq!(book.page, 90);
            assert_eq!(book.last_read_ms, 700);
        }
        // The duplicate's own name is a value, not a gap: the shared read
        // fills the first row's title and leaves the second row's alone.
        assert_eq!(books[0].title.as_deref(), Some("Dune"));
        assert_eq!(books[1].title.as_deref(), Some("dune_1"));
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2,
            "one measurement heals every row at the address"
        );
        assert!(books.iter().all(|b| !b.fp_pending && b.fp == fp(20, 2, 8)));
    }

    #[test]
    fn a_relink_heals_a_missing_book() {
        let mut books = vec![Book {
            missing: true,
            ..linked("a", "/gone/one.pdf")
        }];
        assert!(crate::ledger::relink(&mut books, "a", "/books/one.pdf"));
        assert!(!books[0].missing);
        assert_eq!(books[0].path(), "/books/one.pdf");
    }

    fn check(path: &str, exists: bool, size: u64, mtime: u64, head: u32) -> crate::wire::PathCheck {
        crate::wire::PathCheck {
            path: path.to_string(),
            exists,
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    #[test]
    fn the_same_content_is_the_same_book_whatever_its_address() {
        let mut books = vec![linked("a", "/books/one.pdf")];
        let twin = Book {
            id: "twin".into(),
            origin: Origin::Linked {
                src: "/other/one.pdf".into(),
            },
            ..linked("a", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, twin), "a");
        assert_eq!(books.len(), 1, "a second address never makes a second book");
    }

    #[test]
    fn different_content_is_a_different_book() {
        let mut books = vec![linked("a", "/books/one.pdf")];
        let other = Book {
            fp: fp(11, 1, 7),
            ..linked("b", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, other), "b");
        assert_eq!(books.len(), 2);
    }

    #[test]
    fn an_import_files_in_the_order_the_walk_produced() {
        // One insert at the front per file would reverse a whole folder, which is
        // the difference between a shelf that reads like the folder and one that
        // reads like its mirror.
        let mut books: Vec<Book> = Vec::new();
        for name in ["a", "b", "c", "d"] {
            let book = Book {
                fp: fp(name.as_bytes()[0] as u64, 1, 1),
                ..linked(name, &format!("/books/{name}.pdf"))
            };
            add_book(&mut books, book);
        }
        let ids: Vec<&str> = books.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn removing_returns_the_book_so_the_caller_can_finish_the_job() {
        let mut books = vec![linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")];
        let gone = remove_book(&mut books, "a").expect("present");
        assert_eq!(gone.path(), "/books/one.pdf");
        assert_eq!(books.len(), 1);
        assert!(remove_book(&mut books, "zzz").is_none());
    }

    #[test]
    fn the_resume_point_is_looked_up_by_address() {
        let books = vec![Book {
            page: 42,
            num_pages: 100,
            fraction: Some(0.5),
            ..linked("a", "/books/one.pdf")
        }];
        assert_eq!(find_page(&books, "/books/one.pdf"), Some(42));
        assert_eq!(find_page(&books, "/books/zzz.pdf"), None);
        assert_eq!(find_fraction(&books, "/books/one.pdf"), Some(0.5));
        assert_eq!(find_by_path(&books, "/books/one.pdf").map(|b| b.id.as_str()), Some("a"));
    }

    #[test]
    fn sanitize_dedupes_by_id_and_clamps_the_resume() {
        let mut books = vec![
            Book {
                page: 0,
                ..linked("a", "/books/one.pdf")
            },
            // Same id twice: one book, whichever address the second row wore.
            Book {
                origin: Origin::Linked {
                    src: "/copies/one.pdf".into(),
                },
                ..linked("a", "/books/one.pdf")
            },
            // Same CONTENT twice under two ids: the duplicate a reader chose
            // to keep, which a fingerprint dedupe would silently take back.
            Book {
                id: "dup".into(),
                title: Some("one_1".into()),
                ..linked("dup", "/books/one.pdf")
            },
            Book {
                id: "  ".into(),
                ..linked("blank", "/books/three.pdf")
            },
            Book {
                fp: fp(99, 9, 9),
                fraction: Some(2.0),
                ..linked("c", "/books/four.md")
            },
        ];
        sanitize(&mut books);
        let ids: Vec<&str> = books.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "dup", "c"]);
        assert_eq!(books[0].page, 1, "page 0 clamps to 1");
        assert_eq!(books[1].title.as_deref(), Some("one_1"), "a duplicate keeps the name it was minted with");
        assert_eq!(books[2].fraction, None, "an impossible fraction is dropped");
    }

    #[test]
    fn a_duplicate_is_named_by_the_first_free_counter() {
        let in_use: std::collections::HashSet<String> =
            ["Dune", "Dune_1", "Neuromancer"].iter().map(|s| s.to_string()).collect();
        // The industry's counter: the first number nobody wears.
        assert_eq!(duplicate_title("Dune", &in_use), "Dune_2");
        assert_eq!(duplicate_title("Neuromancer", &in_use), "Neuromancer_1");
        // Duplicating a duplicate steps instead of stacking: the counter is
        // not part of the name.
        assert_eq!(duplicate_title("Dune_1", &in_use), "Dune_2");
        let stepped: std::collections::HashSet<String> = ["Dune", "Dune_1", "Dune_2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(duplicate_title("Dune_2", &stepped), "Dune_3");
        // Gaps are filled...
        let gaps: std::collections::HashSet<String> =
            ["Dune", "Dune_2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(duplicate_title("Dune", &gaps), "Dune_1");
        // ...and a blank base still gets a name a shelf can show.
        assert_eq!(duplicate_title("  ", &std::collections::HashSet::new()), "Book_1");
        // The minted name survives the sanitizer's title rule — the exemption
        // in `reader_core::filename` is what makes the round trip honest.
        assert!(reader_core::filename::is_usable_title("Dune_1"));
        assert!(reader_core::filename::is_usable_title(&duplicate_title("dune", &in_use)));
    }

    #[test]
    fn the_storage_cap_evicts_the_least_recently_read() {
        let mut books: Vec<Book> = (0..(BOOKS_CAP + 2))
            .map(|i| {
                Book {
                    last_read_ms: i as u64,
                    ..linked(&format!("b{i}"), &format!("/books/{i}.pdf"))
                }
            })
            .collect();
        sanitize(&mut books);
        assert_eq!(books.len(), BOOKS_CAP);
        assert!(
            books.iter().all(|b| b.last_read_ms >= 2),
            "the two never-read books go first"
        );
    }

    #[test]
    fn a_book_survives_a_round_trip_through_storage() {
        let book = Book {
            id: "b1".into(),
            fp: fp(1024, 1_700_000_000_000, 0xdead_beef),
            title: Some("Dune".into()),
            author: Some("Frank Herbert".into()),
            format: Format::Pdf,
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/store/pdf/dune_b1.pdf".into(),
            },
            added_ms: 5,
            last_read_ms: 9,
            page: 12,
            num_pages: 400,
            fraction: None,
            missing: true,
            fp_pending: false,
        };
        let json = serde_json::to_string(&book).unwrap();
        assert!(json.contains("\"kind\":\"stored\""), "{json}");
        assert!(json.contains("\"format\":\"pdf\""), "{json}");
        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(back, book);
    }

    #[test]
    fn a_blob_from_before_a_field_existed_still_loads() {
        // Every field carries a default, so a row written by an older build
        // (or one hand-edited) loads rather than dropping the library.
        let b: Book = serde_json::from_str(
            r#"{"id":"b1","fp":{"size":1,"mtimeMs":2,"headHash":3},
                "format":"markdown","origin":{"kind":"linked","src":"/n.md"}}"#,
        )
        .unwrap();
        assert_eq!(b.page, 1);
        assert!(!b.missing);
        assert_eq!(b.title, None);
        assert_eq!(b.path(), "/n.md");
    }
}
