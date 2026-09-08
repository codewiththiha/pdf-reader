//! The catalog's SQL: every statement the library runs, as a pure function from a
//! `&Connection` to a `Result`.
//!
//! Pure on purpose, and it is what makes this testable. A repository function that
//! takes a connection and answers a value can be driven from an in-memory database
//! in a host test, which is the only reason the two claims this module makes are
//! claims rather than hopes: that a fingerprint is a book's identity, and that a
//! purge takes everything that belongs to a book with it.
//!
//! Nothing here touches the filesystem. Measuring a file, copying one into the
//! store and deleting a copy all live in `crate::commands::library`; this module is
//! handed the results and writes them down. The split is the same one the import
//! pipeline already draws — the shell does IO, the domain decides — extended one
//! layer down: the domain decides, the shell measures, and the database remembers.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use library_core::book::{Book, Fingerprint, Origin, ReadPoint, stem_of};
use library_core::folder::WatchedFolder;
use library_core::shelf::{Shelf, ShelfKind};
use library_core::wire::{Cover, GlossRow, LibrarySnapshot, SearchHit};
use library_core::Format;

// ---------------------------------------------------------------------------
// The whole library, in one read.
// ---------------------------------------------------------------------------

/// Everything the first paint of the library needs: books in manual order, shelves
/// with their members resolved, and folders with their ledgers.
///
/// One function rather than three commands because the three are one invariant — a
/// shelf member that names no book is a hole in the grid — and a frontend that
/// fetched them separately would have a window in which that was true.
pub fn bootstrap(conn: &Connection) -> Result<LibrarySnapshot, String> {
    Ok(LibrarySnapshot {
        books: books(conn)?,
        shelves: shelves(conn)?,
        folders: folders(conn)?,
    })
}

/// Every book, in the order the reader arranged them.
pub fn books(conn: &Connection) -> Result<Vec<Book>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, fp_size, fp_mtime_ms, fp_head_hash, fp_pending, title, author, format, \
                    origin, path, src_path, added_ms, last_read_ms, last_page, num_pages, \
                    fraction, missing \
             FROM books ORDER BY position, id",
        )
        .map_err(|e| format!("could not prepare the book query: {e}"))?;
    let rows = stmt
        .query_map([], |row| book_from_row(row))
        .map_err(|e| format!("could not query the books: {e}"))?;
    collect(rows, "the books")
}

fn book_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    let origin_name: String = row.get(8)?;
    let path: String = row.get(9)?;
    let src_path: Option<String> = row.get(10)?;
    Ok(Book {
        id: row.get(0)?,
        fp: Fingerprint {
            size: unsigned(row.get::<_, i64>(1)?),
            mtime_ms: unsigned(row.get::<_, i64>(2)?),
            head_hash: unsigned(row.get::<_, i64>(3)?) as u32,
        },
        fp_pending: row.get::<_, i64>(4)? != 0,
        title: row.get(5)?,
        author: row.get(6)?,
        format: format_of_name(&row.get::<_, String>(7)?),
        origin: if origin_name == "stored" {
            Origin::Stored { src: src_path, store: path }
        } else {
            Origin::Linked { src: path }
        },
        added_ms: unsigned(row.get::<_, i64>(11)?),
        last_read_ms: unsigned(row.get::<_, i64>(12)?),
        page: unsigned(row.get::<_, i64>(13)?) as u32,
        num_pages: unsigned(row.get::<_, i64>(14)?) as u32,
        fraction: row.get(15)?,
        missing: row.get::<_, i64>(16)? != 0,
    })
}

/// Every shelf, in order, with its members resolved.
///
/// Ordered by position and not by depth: a child can come out before the parent it
/// is filed in, which is fine because the nesting is a column on the row rather
/// than a shape in the result, and `library_core::shelf::children_of` is what
/// turns one into the other.
pub fn shelves(conn: &Connection) -> Result<Vec<Shelf>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, kind, folder_id, rel, position, parent \
             FROM shelves ORDER BY position, id",
        )
        .map_err(|e| format!("could not prepare the shelf query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            let kind_name: String = row.get(2)?;
            let folder_id: Option<String> = row.get(3)?;
            let rel: Option<String> = row.get(4)?;
            Ok(Shelf {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: match (kind_name.as_str(), folder_id) {
                    ("folder", Some(folder_id)) => ShelfKind::Folder { folder_id, rel },
                    _ => ShelfKind::Virtual,
                },
                // Filled in below: membership is its own table, and reading it per
                // shelf would be one query per shelf for no benefit.
                books: Vec::new(),
                parent: row.get(6)?,
            })
        })
        .map_err(|e| format!("could not query the shelves: {e}"))?;
    let mut out = collect(rows, "the shelves")?;

    let mut members = conn
        .prepare("SELECT shelf_id, book_id FROM shelf_books ORDER BY position, book_id")
        .map_err(|e| format!("could not prepare the membership query: {e}"))?;
    let pairs = members
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| format!("could not query the memberships: {e}"))?;
    for pair in collect(pairs, "the shelf memberships")? {
        if let Some(shelf) = out.iter_mut().find(|s| s.id == pair.0) {
            shelf.books.push(pair.1);
        }
    }
    Ok(out)
}

/// Every watched folder, with its ledger parsed back out of its columns.
pub fn folders(conn: &Connection) -> Result<Vec<WatchedFolder>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, root, opts_json, placed_json, ignored_json, last_seen_json, \
                    shelf_map_json, scanned_ms \
             FROM folders ORDER BY id",
        )
        .map_err(|e| format!("could not prepare the folder query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FolderRow {
                id: row.get(0)?,
                root: row.get(1)?,
                opts_json: row.get(2)?,
                placed_json: row.get(3)?,
                ignored_json: row.get(4)?,
                last_seen_json: row.get(5)?,
                shelf_map_json: row.get(6)?,
                scanned_ms: row.get(7)?,
            })
        })
        .map_err(|e| format!("could not query the folders: {e}"))?;
    let rows = collect(rows, "the folders")?;
    rows.into_iter().map(FolderRow::into_folder).collect()
}

/// A folder row as the database holds it, before the ledger columns are parsed.
struct FolderRow {
    id: String,
    root: String,
    opts_json: String,
    placed_json: String,
    ignored_json: String,
    last_seen_json: String,
    shelf_map_json: String,
    scanned_ms: i64,
}

impl FolderRow {
    /// Back into the ledger. Parsed outside the row mapper so a corrupt column is
    /// an error that names the folder and the half of it that failed, rather than a
    /// conversion error wearing a type it has nothing to do with.
    fn into_folder(self) -> Result<WatchedFolder, String> {
        let id = self.id.as_str();
        Ok(WatchedFolder {
            opts: parse_ledger(id, "options", &self.opts_json)?,
            placed: parse_ledger(id, "placed set", &self.placed_json)?,
            ignored: parse_ledger(id, "tombstones", &self.ignored_json)?,
            shelf_map: parse_ledger(id, "shelf map", &self.shelf_map_json)?,
            last_seen: parse_ledger(id, "last scan", &self.last_seen_json)?,
            scanned_ms: unsigned(self.scanned_ms),
            id: self.id,
            root: self.root,
        })
    }
}

// ---------------------------------------------------------------------------
// Books.
// ---------------------------------------------------------------------------

/// Insert a book. A fingerprint the catalog already holds is an error, not a
/// second row: the UNIQUE constraint is the import ledger's dedupe rule, enforced
/// where no code path can forget it. Callers that expect a repeat ask
/// [`find_by_fp`] first.
pub fn insert_book(conn: &Connection, book: &Book, position: i64) -> Result<(), String> {
    let (origin_name, src_path) = match &book.origin {
        Origin::Linked { .. } => ("linked", None),
        Origin::Stored { src, .. } => ("stored", src.as_deref()),
    };
    conn.execute(
        "INSERT INTO books (id, fp_size, fp_mtime_ms, fp_head_hash, fp_pending, title, author, \
                            stem, format, origin, path, src_path, position, added_ms, \
                            last_read_ms, last_page, num_pages, fraction, missing) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            book.id,
            book.fp.size as i64,
            book.fp.mtime_ms as i64,
            i64::from(book.fp.head_hash),
            book.fp_pending as i64,
            book.title,
            book.author,
            stem_of(book.path()),
            format_name(book.format),
            origin_name,
            book.path(),
            src_path,
            position,
            book.added_ms as i64,
            book.last_read_ms as i64,
            book.page,
            book.num_pages,
            book.fraction,
            book.missing as i64,
        ],
    )
    .map_err(|e| format!("could not insert book {}: {e}", book.id))?;
    Ok(())
}

/// The book a fingerprint resolves to. The ledger's "have I seen this content
/// before?", which the UNIQUE index answers without a scan.
pub fn find_by_fp(conn: &Connection, fp: &Fingerprint) -> Result<Option<Book>, String> {
    conn.query_row(
        "SELECT id, fp_size, fp_mtime_ms, fp_head_hash, fp_pending, title, author, format, \
                origin, path, src_path, added_ms, last_read_ms, last_page, num_pages, fraction, \
                missing \
         FROM books WHERE fp_size = ?1 AND fp_mtime_ms = ?2 AND fp_head_hash = ?3",
        params![fp.size as i64, fp.mtime_ms as i64, i64::from(fp.head_hash)],
        |row| book_from_row(row),
    )
    .optional()
    .map_err(|e| format!("could not look up a fingerprint: {e}"))
}

/// The book an address resolves to.
pub fn find_by_path(conn: &Connection, path: &str) -> Result<Option<Book>, String> {
    conn.query_row(
        "SELECT id, fp_size, fp_mtime_ms, fp_head_hash, fp_pending, title, author, format, \
                origin, path, src_path, added_ms, last_read_ms, last_page, num_pages, fraction, \
                missing \
         FROM books WHERE path = ?1",
        params![path],
        |row| book_from_row(row),
    )
    .optional()
    .map_err(|e| format!("could not look up {path}: {e}"))
}

/// The next free position in the manual order. Appending is the honest place for an
/// import: a folder arrives in the order the walk produced, which is the order the
/// folder itself lists, and filing at the front one book at a time would hand the
/// reader that order backwards.
pub fn next_position(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM books", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|e| format!("could not read the last position: {e}"))
}

/// How many books the catalog holds.
pub fn book_count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM books", [], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("could not count the books: {e}"))
}

/// Re-measure a book: the relink case, and the heal for a row whose fingerprint was
/// a placeholder. Returns whether anything changed, so a startup pass over a
/// healthy library writes nothing.
///
/// The address only moves for a LINKED book. A stored book's address is the app's
/// own copy, which a measurement of it cannot relocate, and its `src_path` is
/// provenance written by a copy and by nothing else — repointing either would
/// quietly turn "the app keeps its own copy" back into "the app reads your folder
/// again".
pub fn refresh_address(
    conn: &Connection,
    id: &str,
    path: &str,
    fp: &Fingerprint,
) -> Result<bool, String> {
    let changed = conn
        .execute(
            "UPDATE books SET \
                    path = CASE WHEN origin = 'linked' THEN ?2 ELSE path END, \
                    stem = CASE WHEN origin = 'linked' THEN ?6 ELSE stem END, \
                    fp_size = ?3, fp_mtime_ms = ?4, fp_head_hash = ?5, fp_pending = 0, \
                    missing = 0 \
             WHERE id = ?1 AND (path <> ?2 OR fp_size <> ?3 OR fp_mtime_ms <> ?4 \
                    OR fp_head_hash <> ?5 OR fp_pending <> 0 OR missing <> 0)",

            params![
                id,
                path,
                fp.size as i64,
                fp.mtime_ms as i64,
                i64::from(fp.head_hash),
                stem_of(path),
            ],
        )
        .map_err(|e| format!("could not relink {id}: {e}"))?;
    Ok(changed > 0)
}

/// Mark a book's address as resolving or not. A missing book keeps its row, its
/// resume point and every shelf it is on: the reader moved a folder, and what they
/// want back is the page they were on.
pub fn set_missing(conn: &Connection, id: &str, missing: bool) -> Result<bool, String> {
    let changed = conn
        .execute(
            "UPDATE books SET missing = ?2 WHERE id = ?1 AND missing <> ?2",
            params![id, missing as i64],
        )
        .map_err(|e| format!("could not mark {id} missing: {e}"))?;
    Ok(changed > 0)
}

/// Record a read: the resume point, the stamp, and a title or author that only ever
/// fills a gap — so the name a document gave at first open survives every later
/// resume, and a scan that knows neither cannot blank what an open learned.
pub fn touch_read(
    conn: &Connection,
    id: &str,
    title: Option<&str>,
    author: Option<&str>,
    point: ReadPoint,
    now_ms: u64,
) -> Result<bool, String> {
    let point = settle(point);
    let changed = conn
        .execute(
            "UPDATE books SET last_page = ?2, num_pages = ?3, fraction = ?4, last_read_ms = ?5, \
                    missing = 0, \
                    title = COALESCE(NULLIF(TRIM(title), ''), ?6), \
                    author = COALESCE(NULLIF(TRIM(author), ''), ?7) \
             WHERE id = ?1",
            params![
                id,
                point.page,
                point.num_pages,
                point.fraction,
                now_ms as i64,
                title.map(str::trim).filter(|t| !t.is_empty()),
                author.map(str::trim).filter(|a| !a.is_empty()),
            ],
        )
        .map_err(|e| format!("could not record a read of {id}: {e}"))?;
    Ok(changed > 0)
}

/// The settled form of a resume point, so no writer can hand the catalog a page of
/// zero or a fraction past the end. One definition, in the crate that owns the
/// type, would be better still — but `ReadPoint`'s settle is private to it and the
/// database is the layer that has to be right.
fn settle(point: ReadPoint) -> ReadPoint {
    ReadPoint {
        page: point.page.max(1),
        num_pages: point.num_pages,
        fraction: point.fraction.filter(|f| (0.0..=1.0).contains(f)),
    }
}

/// Rewrite the manual order from a list of ids. Positions are dense and start at
/// zero, so "the order" is one column and a drag is one statement per row rather
/// than a fractional-index scheme that eventually needs a rebalance.
pub fn reorder(conn: &Connection, ids: &[String]) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("could not begin a reorder: {e}"))?;
    for (position, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE books SET position = ?2 WHERE id = ?1",
            params![id, position as i64],
        )
        .map_err(|e| format!("could not move {id} to {position}: {e}"))?;
    }
    tx.commit().map_err(|e| format!("could not commit a reorder: {e}"))
}

/// Delete a book, and return the address of the copy the app made — if it made one.
///
/// One statement, because the schema does the rest: `ON DELETE CASCADE` takes the
/// highlights, the cached answers, the cover and every shelf membership with the
/// row. The store path is read BEFORE the delete, since the row is the only place
/// it is written down, and handed back rather than removed here: this module does
/// not touch the filesystem, and the caller must only delete a path that
/// canonicalises inside the app's own store.
pub fn purge(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("could not begin a purge: {e}"))?;
    let stored: Option<String> = tx
        .query_row(
            "SELECT path FROM books WHERE id = ?1 AND origin = 'stored'",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("could not read the store path of {id}: {e}"))?;
    tx.execute("DELETE FROM books WHERE id = ?1", params![id])
        .map_err(|e| format!("could not remove {id}: {e}"))?;
    tx.commit().map_err(|e| format!("could not commit a purge: {e}"))?;
    Ok(stored)
}

// ---------------------------------------------------------------------------
// Shelves and membership.
// ---------------------------------------------------------------------------

/// Add a shelf, at the level its `parent` names.
///
/// The parent is written as the value it is rather than checked against the table:
/// there is no foreign key on the column (see `migrations/0003_shelf_parent.sql`),
/// so shelves can be restored in any order and a dangling parent is
/// `library_core::shelf::sanitize`'s to collapse back to the root.
pub fn shelf_insert(conn: &Connection, shelf: &Shelf, position: i64) -> Result<(), String> {
    let (kind, folder_id, rel) = match &shelf.kind {
        ShelfKind::Virtual => ("virtual", None, None),
        ShelfKind::Folder { folder_id, rel } => ("folder", Some(folder_id.as_str()), rel.as_deref()),
    };
    conn.execute(
        "INSERT INTO shelves (id, name, kind, folder_id, rel, position, parent) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            shelf.id,
            shelf.name,
            kind,
            folder_id,
            rel,
            position,
            shelf.parent
        ],
    )
    .map_err(|e| format!("could not create shelf {}: {e}", shelf.id))?;
    Ok(())
}

/// File one shelf inside another, or back out to the top level when `parent` is
/// `None`. True when the row's parent was not already that.
///
/// The cycle rule is NOT here. It is `library_core::shelf::can_nest`, asked by the
/// caller before it gets to this: a graph walk rewritten as a recursive CTE would
/// be a second answer to the same question in the one language the other cannot
/// read, and the two would disagree silently.
pub fn shelf_reparent(conn: &Connection, id: &str, parent: Option<&str>) -> Result<bool, String> {
    // `IS NOT` rather than `<>`, because the top level is a NULL and `NULL <> x`
    // is NULL — an UPDATE guarded by it would never fire on a shelf being lifted
    // out of a folder, and would report "changed" for one that had not moved.
    let changed = conn
        .execute(
            "UPDATE shelves SET parent = ?2 WHERE id = ?1 AND parent IS NOT ?2",
            params![id, parent],
        )
        .map_err(|e| format!("could not re-file shelf {id}: {e}"))?;
    Ok(changed > 0)
}

/// Rename a shelf. A blank name is refused rather than stored: a crumb with
/// nothing on it is a crumb the reader cannot click.
pub fn shelf_rename(conn: &Connection, id: &str, name: &str) -> Result<bool, String> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(false);
    }
    let changed = conn
        .execute(
            "UPDATE shelves SET name = ?2 WHERE id = ?1 AND name <> ?2",
            params![id, name],
        )
        .map_err(|e| format!("could not rename shelf {id}: {e}"))?;
    Ok(changed > 0)
}

/// Take a shelf apart. Its memberships cascade; its books do not, because a shelf
/// is a list of ids and never held a byte.
///
/// The shelves inside it move up to the level it was on, which is
/// `library_core::shelf::lift_children` in SQL and for the same reason: a child
/// left pointing at a parent that is gone renders on no level at all, and a reader
/// who removed one folder did not ask to lose the folders filed in it. One
/// transaction, because the lift and the delete are one rule — a lift that ran
/// after the delete would have nothing to read the inherited level from.
pub fn shelf_delete(conn: &Connection, id: &str) -> Result<bool, String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("could not begin removing shelf {id}: {e}"))?;
    tx.execute(
        "UPDATE shelves SET parent = (SELECT parent FROM shelves WHERE id = ?1) WHERE parent = ?1",
        params![id],
    )
    .map_err(|e| format!("could not lift the shelves inside {id}: {e}"))?;
    let removed = tx
        .execute("DELETE FROM shelves WHERE id = ?1", params![id])
        .map_err(|e| format!("could not remove shelf {id}: {e}"))?;
    tx.commit()
        .map_err(|e| format!("could not commit removing shelf {id}: {e}"))?;
    Ok(removed > 0)
}

/// The next free position on one shelf.
pub fn shelf_next_position(conn: &Connection, shelf_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM shelf_books WHERE shelf_id = ?1",
        params![shelf_id],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| format!("could not read the end of shelf {shelf_id}: {e}"))
}

/// File a book on a shelf, unless it is already on it. A restore and an "also show
/// it here" both add a book that may already be a member, and appending it again
/// would move a book the reader can see to the end of a shelf for no reason.
pub fn shelf_add(conn: &Connection, shelf_id: &str, book_id: &str) -> Result<(), String> {
    let position = shelf_next_position(conn, shelf_id)?;
    conn.execute(
        "INSERT INTO shelf_books (shelf_id, book_id, position) VALUES (?1, ?2, ?3) \
         ON CONFLICT (shelf_id, book_id) DO NOTHING",
        params![shelf_id, book_id, position],
    )
    .map_err(|e| format!("could not file {book_id} on {shelf_id}: {e}"))?;
    Ok(())
}

/// Take a book off one shelf.
pub fn shelf_forget(conn: &Connection, shelf_id: &str, book_id: &str) -> Result<bool, String> {
    let removed = conn
        .execute(
            "DELETE FROM shelf_books WHERE shelf_id = ?1 AND book_id = ?2",
            params![shelf_id, book_id],
        )
        .map_err(|e| format!("could not unfile {book_id} from {shelf_id}: {e}"))?;
    Ok(removed > 0)
}

/// Move a book to `to` at `index`, off `from` when the two differ.
///
/// Rewrites the target shelf's positions rather than squeezing a fractional index
/// between two neighbours: a membership list is short, positions are dense, and a
/// scheme that never rewrites eventually needs a rebalance that nobody scheduled.
pub fn shelf_move(
    conn: &Connection,
    book_id: &str,
    from: Option<&str>,
    to: &str,
    index: Option<i64>,
) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("could not begin a move: {e}"))?;
    if let Some(from) = from.filter(|id| *id != to) {
        tx.execute(
            "DELETE FROM shelf_books WHERE shelf_id = ?1 AND book_id = ?2",
            params![from, book_id],
        )
        .map_err(|e| format!("could not unfile {book_id}: {e}"))?;
    }
    let mut stmt = tx
        .prepare("SELECT book_id FROM shelf_books WHERE shelf_id = ?1 AND book_id <> ?2 ORDER BY position, book_id")
        .map_err(|e| format!("could not read shelf {to}: {e}"))?;
    let rows = stmt
        .query_map(params![to, book_id], |row| row.get::<_, String>(0))
        .map_err(|e| format!("could not read shelf {to}: {e}"))?;
    let mut members = collect(rows, "a shelf's members")?;
    drop(stmt);
    let at = match index {
        Some(i) if i >= 0 => (i as usize).min(members.len()),
        _ => members.len(),
    };
    members.insert(at, book_id.to_string());
    tx.execute(
        "DELETE FROM shelf_books WHERE shelf_id = ?1",
        params![to],
    )
    .map_err(|e| format!("could not clear shelf {to}: {e}"))?;
    for (position, member) in members.iter().enumerate() {
        tx.execute(
            "INSERT INTO shelf_books (shelf_id, book_id, position) VALUES (?1, ?2, ?3)",
            params![to, member, position as i64],
        )
        .map_err(|e| format!("could not refile {member}: {e}"))?;
    }
    tx.commit().map_err(|e| format!("could not commit a move: {e}"))
}

// ---------------------------------------------------------------------------
// Watched folders.
// ---------------------------------------------------------------------------

/// Write a folder and its ledger. Replaces rather than merges: the ledger is one
/// aggregate that is always read and written whole with its folder, and a merge
/// would be a second opinion about which half of it is newer.
pub fn folder_upsert(conn: &Connection, folder: &WatchedFolder) -> Result<(), String> {

    conn.execute(
        "INSERT INTO folders (id, root, opts_json, placed_json, ignored_json, last_seen_json, \
                              shelf_map_json, scanned_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT (id) DO UPDATE SET root = excluded.root, opts_json = excluded.opts_json, \
             placed_json = excluded.placed_json, ignored_json = excluded.ignored_json, \
             last_seen_json = excluded.last_seen_json, shelf_map_json = excluded.shelf_map_json, \
             scanned_ms = excluded.scanned_ms",
        params![
            folder.id,
            folder.root,
            ledger_json("the folder options", &folder.opts)?,
            ledger_json("the placed set", &folder.placed)?,
            ledger_json("the tombstones", &folder.ignored)?,
            ledger_json("the last scan", &folder.last_seen)?,
            ledger_json("the shelf map", &folder.shelf_map)?,
            folder.scanned_ms as i64,
        ],
    )
    .map_err(|e| format!("could not write folder {}: {e}", folder.id))?;
    Ok(())
}

/// Forget a watched folder. Its shelves are NOT cascaded: a shelf is the reader's
/// arrangement and outlives the watch that produced it, and a folder the reader
/// stops watching should not take their filing with it.
pub fn folder_delete(conn: &Connection, id: &str) -> Result<bool, String> {
    let removed = conn
        .execute("DELETE FROM folders WHERE id = ?1", params![id])
        .map_err(|e| format!("could not forget folder {id}: {e}"))?;
    Ok(removed > 0)
}

// ---------------------------------------------------------------------------
// Covers, highlights and cached answers.
// ---------------------------------------------------------------------------

/// Store a book's cover, replacing any it had.
pub fn cover_put(conn: &Connection, book_id: &str, cover: &Cover) -> Result<(), String> {
    conn.execute(
        "INSERT INTO covers (book_id, width, height, data_url) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (book_id) DO UPDATE SET width = excluded.width, height = excluded.height, \
             data_url = excluded.data_url",
        params![book_id, cover.width, cover.height, cover.data_url],
    )
    .map_err(|e| format!("could not store the cover of {book_id}: {e}"))?;
    Ok(())
}

/// One book's cover. Read per card rather than at boot: the cover map is the largest
/// thing the library holds, and a shelf of two hundred books does not need two
/// hundred data URLs in memory to draw its first row.
pub fn cover_get(conn: &Connection, book_id: &str) -> Result<Option<Cover>, String> {
    conn.query_row(
        "SELECT width, height, data_url FROM covers WHERE book_id = ?1",
        params![book_id],
        |row| {
            Ok(Cover {
                width: row.get(0)?,
                height: row.get(1)?,
                data_url: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("could not read the cover of {book_id}: {e}"))
}

/// Replace a book's highlights with `marks`.
///
/// A replace and not a merge, because the frontend owns the live set and writes it
/// whole: a merge would need the database to know which mark superseded which, and
/// the answer to that is in the reader's click order, not in a table. The cached
/// answers go with their marks — `ON DELETE CASCADE` on `gloss_answers` — so a mark
/// that stops existing cannot leave an answer nobody can reach.
pub fn gloss_save(conn: &Connection, book_id: &str, marks: &[GlossRow]) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("could not begin a highlight write: {e}"))?;
    tx.execute("DELETE FROM gloss_marks WHERE book_id = ?1", params![book_id])
        .map_err(|e| format!("could not clear the highlights of {book_id}: {e}"))?;
    for mark in marks {
        tx.execute(
            "INSERT INTO gloss_marks (id, book_id, page, word, context, anchor_json, created_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                mark.id,
                book_id,
                mark.page,
                mark.word,
                mark.context,
                mark.anchor_json,
                mark.created_ms as i64,
            ],
        )
        .map_err(|e| format!("could not store a highlight of {book_id}: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("could not commit the highlights of {book_id}: {e}"))
}

/// A book's highlights, oldest first.
pub fn gloss_load(conn: &Connection, book_id: &str) -> Result<Vec<GlossRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, book_id, page, word, context, anchor_json, created_ms \
             FROM gloss_marks WHERE book_id = ?1 ORDER BY created_ms, id",
        )
        .map_err(|e| format!("could not prepare the highlight query: {e}"))?;
    let rows = stmt
        .query_map(params![book_id], |row| {
            Ok(GlossRow {
                id: row.get(0)?,
                book_id: row.get(1)?,
                page: row.get(2)?,
                word: row.get(3)?,
                context: row.get(4)?,
                anchor_json: row.get(5)?,
                created_ms: unsigned(row.get::<_, i64>(6)?),
            })
        })
        .map_err(|e| format!("could not query the highlights of {book_id}: {e}"))?;
    collect(rows, "the highlights")
}

/// Cache an answer against the mark that asked for it.
///
/// The value is opaque JSON on purpose. `WordInfo` lives in a crate the shell
/// cannot depend on, and the database has no business parsing a sentence: it stores
/// the bytes and hands them back, which is also what makes the cache survive a
/// change to the answer's shape without a migration.
pub fn answer_put(conn: &Connection, mark_id: &str, info_json: &str, saved_ms: u64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO gloss_answers (mark_id, info_json, saved_ms) VALUES (?1, ?2, ?3) \
         ON CONFLICT (mark_id) DO UPDATE SET info_json = excluded.info_json, \
             saved_ms = excluded.saved_ms",
        params![mark_id, info_json, saved_ms as i64],
    )
    .map_err(|e| format!("could not cache the answer for {mark_id}: {e}"))?;
    Ok(())
}

/// The cached answer for a mark, if there is one.
pub fn answer_get(conn: &Connection, mark_id: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT info_json FROM gloss_answers WHERE mark_id = ?1",
        params![mark_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("could not read the answer for {mark_id}: {e}"))
}

// ---------------------------------------------------------------------------
// Search.
// ---------------------------------------------------------------------------

/// The books a query matches, best first.
///
/// An empty answer for an empty query is the whole of the "blank bar means show
/// everything" rule: the caller does not filter, it asks, and asking nothing is
/// asking for nothing.
pub fn search(conn: &Connection, query: &str, limit: u32) -> Result<Vec<SearchHit>, String> {
    let pattern = fts_query(query);
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    // Not aliased: FTS5 resolves the left operand of MATCH against the table
    // name, so `FROM books_fts AS f WHERE f MATCH ?` reads `f` as a column that
    // does not exist. `bm25()` is spelled out rather than selected as `rank`
    // because the fts table also has a hidden column of that name, and an ORDER BY
    // that could resolve to either is an ORDER BY nobody can predict.
    let mut stmt = conn
        .prepare(
            "SELECT b.id, bm25(books_fts) FROM books_fts \
             JOIN books AS b ON b.rowid = books_fts.rowid \
             WHERE books_fts MATCH ?1 AND b.missing = 0 \
             ORDER BY bm25(books_fts) LIMIT ?2",
        )
        .map_err(|e| format!("could not prepare the search: {e}"))?;
    let rows = stmt
        .query_map(params![pattern, i64::from(limit)], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                rank: row.get(1)?,
                snippet: None,
            })
        })
        .map_err(|e| format!("could not search for {query:?}: {e}"))?;
    collect(rows, "the search results")
}

/// A query string into an FTS5 pattern: every term quoted and prefix-matched, so
/// `rus servers` finds "Rust Programming" and a term with a quote in it cannot
/// become syntax.
///
/// A term with nothing alphanumeric in it is dropped rather than passed through —
/// `"-"*` is a syntax error to the tokenizer, and a search box that errors on a
/// stray hyphen is a search box nobody trusts.
pub fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| term.replace('"', ""))
        .filter(|term| term.chars().any(|c| c.is_alphanumeric()))
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Small shared pieces.
// ---------------------------------------------------------------------------

/// Read a one-off value the migration wrote, or write one.
pub fn kv_get(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM kv WHERE key = ?1", params![key], |row| {
        row.get(0)
    })
    .optional()
    .map_err(|e| format!("could not read {key}: {e}"))
}

pub fn kv_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| format!("could not write {key}: {e}"))?;
    Ok(())
}

/// The column name a format is stored under. Duplicated from the registry's serde
/// names because the shell does not name `reader-core`, and pinned to them by
/// `the_format_column_names_are_the_serde_names` below.
fn format_name(format: Format) -> &'static str {
    match format {
        Format::Pdf => "pdf",
        Format::Text => "text",
        Format::Markdown => "markdown",
    }
}

fn format_of_name(name: &str) -> Format {
    match name {
        "text" => Format::Text,
        "markdown" => Format::Markdown,
        _ => Format::Pdf,
    }
}

/// One of a folder's ledger columns, back into its type. A generic function rather
/// than a closure because a closure has exactly one return type and there are five
/// columns here, each a different one.
fn parse_ledger<T: serde::de::DeserializeOwned>(
    folder_id: &str,
    what: &str,
    json: &str,
) -> Result<T, String> {
    serde_json::from_str(json).map_err(|e| format!("folder {folder_id}: corrupt {what}: {e}"))
}

/// One of a folder's ledger columns, as JSON. A function rather than a closure
/// because the value is generic and a closure parameter cannot be.
fn ledger_json<T: Serialize>(what: &str, value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("could not write {what}: {e}"))
}

/// A count stored as a signed integer, read back as the unsigned one the domain
/// uses. Clamped rather than wrapped: a negative would mean a corrupt row, and a
/// zero is a lie the rest of the code can live with.
fn unsigned(value: i64) -> u64 {
    value.max(0) as u64
}

/// A mapped-row iterator into a `Vec`, with the error named.
fn collect<T>(rows: impl Iterator<Item = Result<T, rusqlite::Error>>, what: &str) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("could not read {what}: {e}"))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{configure, migrate};
    use library_core::folder::{FolderOpts, Tombstone};
    use std::collections::{BTreeMap, HashSet};

    /// An in-memory catalog running the real migrations. Every test below is
    /// against the schema the app ships, not against a copy of it.
    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        configure(&conn).expect("pragmas");
        migrate(&conn).expect("migrations");
        conn
    }

    fn fp(size: u64, mtime: u64, head: u32) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    fn book(id: &str, path: &str, fingerprint: Fingerprint) -> Book {
        Book {
            id: id.to_string(),
            fp: fingerprint,
            title: None,
            author: None,
            format: Format::Pdf,
            origin: Origin::Linked {
                src: path.to_string(),
            },
            added_ms: 10,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
        }
    }

    /// The catalog with one linked book in it, which is most tests' starting point.
    fn one_book(conn: &Connection) -> Book {
        let dune = Book {
            title: Some("Dune".into()),
            author: Some("Frank Herbert".into()),
            page: 42,
            num_pages: 400,
            ..book("b1", "/books/dune.pdf", fp(1024, 7, 99))
        };
        insert_book(conn, &dune, 0).expect("inserted");
        dune
    }

    fn cover() -> Cover {
        Cover {
            width: 240.0,
            height: 320.0,
            data_url: "data:image/jpeg;base64,AAAA".to_string(),
        }
    }

    fn mark(id: &str, word: &str, context: &str) -> GlossRow {
        GlossRow {
            id: id.to_string(),
            book_id: "b1".to_string(),
            page: 12,
            word: word.to_string(),
            context: context.to_string(),
            anchor_json: "{\"x\":1}".to_string(),
            created_ms: 5,
        }
    }

    // ---------------------------------------------------------------- identity --

    #[test]
    fn a_book_survives_a_round_trip_through_its_columns() {
        let conn = db();
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/LibraryStore/b2.pdf".into(),
            },
            fp_pending: true,
            missing: true,
            fraction: Some(0.25),
            last_read_ms: 99,
            ..book("b2", "/app/LibraryStore/b2.pdf", fp(10, 20, 30))
        };
        insert_book(&conn, &stored, 3).expect("inserted");
        let back = books(&conn).expect("read back");
        assert_eq!(back, vec![stored], "every column is part of the book");
        // The address a stored book answers with is the copy, and the source it
        // came from survives beside it as provenance.
        assert_eq!(back[0].path(), "/app/LibraryStore/b2.pdf");
        assert_eq!(back[0].origin.source(), Some("/downloads/dune.pdf"));
    }

    #[test]
    fn one_file_is_one_book_and_the_database_is_what_says_so() {
        // The import ledger's dedupe rule, moved out of application code: a second
        // insert of the same fingerprint is a constraint violation, so no code path
        // written later can add a duplicate by forgetting to ask.
        let conn = db();
        one_book(&conn);
        let twin = book("b2", "/copies/dune.pdf", fp(1024, 7, 99));
        let error = insert_book(&conn, &twin, 1).unwrap_err();
        assert!(error.contains("UNIQUE"), "{error}");
        assert_eq!(book_count(&conn).unwrap(), 1);
        assert_eq!(find_by_fp(&conn, &fp(1024, 7, 99)).unwrap().map(|b| b.id).as_deref(), Some("b1"));
    }

    #[test]
    fn a_fingerprint_that_differs_by_one_field_is_another_book() {
        let conn = db();
        one_book(&conn);
        for fingerprint in [fp(1025, 7, 99), fp(1024, 8, 99), fp(1024, 7, 100)] {
            assert!(
                find_by_fp(&conn, &fingerprint).unwrap().is_none(),
                "{fingerprint:?} must not resolve to dune"
            );
        }
    }

    #[test]
    fn a_placeholder_fingerprint_is_unique_per_address() {
        // A migrated row has never been measured, so its fingerprint is derived
        // from its address. Two such rows must not collide, or migrating a library
        // would empty it.
        let conn = db();
        for (index, path) in ["/books/a.pdf", "/books/b.pdf", "/books/c.pdf"]
            .into_iter()
            .enumerate()
        {
            let row = book(
                &format!("m{index}"),
                path,
                Fingerprint::placeholder(path),
            );
            insert_book(&conn, &row, index as i64).expect("each address is its own placeholder");
        }
        assert_eq!(book_count(&conn).unwrap(), 3);
    }

    // ------------------------------------------------------------------- resume --

    #[test]
    fn a_read_writes_the_resume_point_and_stamps_the_book() {
        let conn = db();
        one_book(&conn);
        assert!(touch_read(
            &conn,
            "b1",
            None,
            None,
            ReadPoint { page: 77, num_pages: 400, fraction: None },
            500
        )
        .unwrap());
        let back = find_by_path(&conn, "/books/dune.pdf").unwrap().expect("present");
        assert_eq!(back.page, 77);
        assert_eq!(back.last_read_ms, 500);
    }

    #[test]
    fn an_impossible_resume_point_is_settled_on_the_way_in() {
        let conn = db();
        one_book(&conn);
        touch_read(
            &conn,
            "b1",
            None,
            None,
            ReadPoint { page: 0, num_pages: 9, fraction: Some(1.7) },
            1,
        )
        .unwrap();
        let back = find_by_path(&conn, "/books/dune.pdf").unwrap().expect("present");
        assert_eq!(back.page, 1, "there is no page zero to resume at");
        assert_eq!(back.fraction, None, "a fraction past the end is not a position");
    }

    #[test]
    fn a_title_and_author_only_ever_fill_a_gap() {
        let conn = db();
        one_book(&conn);
        touch_read(&conn, "b1", Some("dune.pdf"), Some("Somebody Else"), ReadPoint::fresh(), 1)
            .unwrap();
        let back = find_by_path(&conn, "/books/dune.pdf").unwrap().expect("present");
        assert_eq!(back.title.as_deref(), Some("Dune"));
        assert_eq!(back.author.as_deref(), Some("Frank Herbert"));

        // A book with neither takes both.
        let bare = book("b3", "/books/bare.pdf", fp(1, 1, 1));
        insert_book(&conn, &bare, 1).unwrap();
        touch_read(&conn, "b3", Some("Bare"), Some("Nobody"), ReadPoint::fresh(), 2).unwrap();
        let back = find_by_path(&conn, "/books/bare.pdf").unwrap().expect("present");
        assert_eq!(back.title.as_deref(), Some("Bare"));
        assert_eq!(back.author.as_deref(), Some("Nobody"));
    }

    // ------------------------------------------------------------------- relink --

    #[test]
    fn a_relink_moves_the_address_and_keeps_everything_else() {
        let conn = db();
        one_book(&conn);
        cover_put(&conn, "b1", &cover()).unwrap();
        gloss_save(&conn, "b1", &[mark("m1", "palimpsest", "a palimpsest of drafts")]).unwrap();

        assert!(refresh_address(&conn, "b1", "/moved/dune.pdf", &fp(2048, 8, 7)).unwrap());
        let back = find_by_path(&conn, "/moved/dune.pdf").unwrap().expect("present");
        assert_eq!(back.id, "b1", "the id is the book, the address is not");
        assert_eq!(back.page, 42, "the resume point is the reader's, not the scan's");
        assert_eq!(back.fp, fp(2048, 8, 7));
        assert!(!back.fp_pending);
        assert!(find_by_path(&conn, "/books/dune.pdf").unwrap().is_none());
        // The highlights and the cover are keyed by id, which is the whole reason a
        // moved file keeps them.
        assert_eq!(gloss_load(&conn, "b1").unwrap().len(), 1);
        assert!(cover_get(&conn, "b1").unwrap().is_some());
        // Telling it the same thing again is not a change.
        assert!(!refresh_address(&conn, "b1", "/moved/dune.pdf", &fp(2048, 8, 7)).unwrap());
    }

    #[test]
    fn a_re_measurement_never_moves_a_stored_book_or_rewrites_its_provenance() {
        let conn = db();
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/LibraryStore/b1.pdf".into(),
            },
            ..book("b1", "/app/LibraryStore/b1.pdf", fp(1, 1, 1))
        };
        insert_book(&conn, &stored, 0).unwrap();
        refresh_address(&conn, "b1", "/elsewhere/dune.pdf", &fp(2, 2, 2)).unwrap();
        let back = books(&conn).unwrap().remove(0);
        assert_eq!(back.path(), "/app/LibraryStore/b1.pdf", "the copy is the address");
        assert_eq!(
            back.origin.source(),
            Some("/downloads/dune.pdf"),
            "provenance is written by a copy and by nothing else"
        );
        assert_eq!(back.fp, fp(2, 2, 2), "the fingerprint is still refreshed");
    }

    #[test]
    fn marking_a_book_missing_keeps_the_row() {
        let conn = db();
        one_book(&conn);
        assert!(set_missing(&conn, "b1", true).unwrap());
        let back = find_by_path(&conn, "/books/dune.pdf").unwrap().expect("still there");
        assert!(back.missing);
        assert_eq!(back.page, 42, "and keeps the page the reader was on");
        assert!(!set_missing(&conn, "b1", true).unwrap(), "twice is not news");
        assert!(set_missing(&conn, "b1", false).unwrap());
    }

    // -------------------------------------------------------------------- purge --

    #[test]
    fn a_purge_takes_everything_that_belongs_to_the_book() {
        let conn = db();
        one_book(&conn);
        let other = book("b2", "/books/other.pdf", fp(2, 2, 2));
        insert_book(&conn, &other, 1).unwrap();
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        shelf_add(&conn, "s1", "b1").unwrap();
        shelf_add(&conn, "s1", "b2").unwrap();
        cover_put(&conn, "b1", &cover()).unwrap();
        gloss_save(&conn, "b1", &[mark("m1", "palimpsest", "a palimpsest of drafts")]).unwrap();
        answer_put(&conn, "m1", "{\"definition\":\"x\"}", 1).unwrap();

        let stored = purge(&conn, "b1").unwrap();
        assert_eq!(stored, None, "a linked book has no copy to delete");
        assert_eq!(book_count(&conn).unwrap(), 1);
        assert!(cover_get(&conn, "b1").unwrap().is_none());
        assert!(gloss_load(&conn, "b1").unwrap().is_empty());
        assert!(answer_get(&conn, "m1").unwrap().is_none(), "the answer goes with its mark");
        let shelves = shelves(&conn).unwrap();
        assert_eq!(shelves[0].books, vec!["b2".to_string()], "the shelf survives, the membership does not");
    }

    #[test]
    fn a_purge_hands_back_the_copy_so_the_caller_can_delete_it() {
        // The database does not touch the filesystem, so it answers with the
        // address and the caller applies the store-root guard it already has.
        let conn = db();
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/LibraryStore/b1.pdf".into(),
            },
            ..book("b1", "/app/LibraryStore/b1.pdf", fp(1, 1, 1))
        };
        insert_book(&conn, &stored, 0).unwrap();
        assert_eq!(
            purge(&conn, "b1").unwrap().as_deref(),
            Some("/app/LibraryStore/b1.pdf")
        );
        assert_eq!(purge(&conn, "b1").unwrap(), None, "a second purge removes nothing");
    }

    #[test]
    fn purging_a_book_nobody_has_is_not_an_error() {
        let conn = db();
        assert_eq!(purge(&conn, "nope").unwrap(), None);
    }

    // -------------------------------------------------------------------- order --

    #[test]
    fn the_manual_order_is_written_and_read_back() {
        let conn = db();
        for index in 0..3u64 {
            let row = book(
                &format!("b{index}"),
                &format!("/books/{index}.pdf"),
                fp(index, index, index as u32),
            );
            insert_book(&conn, &row, index as i64).unwrap();
        }
        assert_eq!(next_position(&conn).unwrap(), 3);
        let ids: Vec<String> = books(&conn).unwrap().into_iter().map(|b| b.id).collect();
        assert_eq!(ids, vec!["b0", "b1", "b2"]);

        reorder(&conn, &["b2".into(), "b0".into(), "b1".into()]).unwrap();
        let ids: Vec<String> = books(&conn).unwrap().into_iter().map(|b| b.id).collect();
        assert_eq!(ids, vec!["b2", "b0", "b1"], "a drag is the whole order, rewritten");
    }

    // ------------------------------------------------------------------- shelves --

    #[test]
    fn a_shelf_and_its_members_round_trip() {
        let conn = db();
        one_book(&conn);
        let folder_shelf = Shelf {
            id: "s2".into(),
            name: "scifi".into(),
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            books: Vec::new(),
            parent: None,
        };
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        shelf_insert(&conn, &folder_shelf, 1).unwrap();
        shelf_add(&conn, "s1", "b1").unwrap();
        shelf_add(&conn, "s2", "b1").unwrap();

        let back = shelves(&conn).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].id, "s1");
        assert_eq!(back[0].kind, ShelfKind::Virtual);
        assert_eq!(back[0].books, vec!["b1".to_string()]);
        assert_eq!(
            back[1].kind,
            ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into())
            }
        );
        // Filing twice is one membership: a restore must not shuffle a shelf the
        // reader can see.
        shelf_add(&conn, "s1", "b1").unwrap();
        assert_eq!(shelves(&conn).unwrap()[0].books.len(), 1);
    }

    #[test]
    fn a_shelf_keeps_the_level_it_was_filed_in() {
        let conn = db();
        shelf_insert(&conn, &virtual_shelf("s1", "Fiction"), 0).unwrap();
        shelf_insert(&conn, &nested_shelf("s2", "Sci-fi", "s1"), 1).unwrap();
        shelf_insert(&conn, &nested_shelf("s3", "Space", "s2"), 2).unwrap();

        let back = shelves(&conn).unwrap();
        assert_eq!(back[0].parent, None);
        assert_eq!(back[1].parent.as_deref(), Some("s1"));
        assert_eq!(back[2].parent.as_deref(), Some("s2"));
        assert_eq!(
            library_core::shelf::children_of(&back, Some("s1"))
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["s2"]
        );
        assert_eq!(
            library_core::shelf::ancestors(&back, "s3")
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["s1", "s2"],
            "the way out of a shelf is the column, read back as a chain"
        );
    }

    #[test]
    fn re_filing_a_shelf_moves_one_column_and_reports_whether_it_moved() {
        let conn = db();
        shelf_insert(&conn, &virtual_shelf("s1", "Fiction"), 0).unwrap();
        shelf_insert(&conn, &virtual_shelf("s2", "Sci-fi"), 1).unwrap();

        assert!(shelf_reparent(&conn, "s2", Some("s1")).unwrap());
        assert_eq!(shelves(&conn).unwrap()[1].parent.as_deref(), Some("s1"));
        assert!(
            !shelf_reparent(&conn, "s2", Some("s1")).unwrap(),
            "filing a shelf where it already is changed nothing"
        );
        // Back out to the top level, which is a NULL and not an empty string:
        // `children_of(None)` is the root, and a root shelf whose parent reads
        // back as Some("") is a shelf on no level at all.
        assert!(shelf_reparent(&conn, "s2", None).unwrap());
        assert_eq!(shelves(&conn).unwrap()[1].parent, None);
        assert!(!shelf_reparent(&conn, "s2", None).unwrap());
        // A shelf that is not there is not a move.
        assert!(!shelf_reparent(&conn, "nope", Some("s1")).unwrap());
    }

    #[test]
    fn removing_a_shelf_lifts_the_shelves_inside_it_to_its_own_level() {
        let conn = db();
        shelf_insert(&conn, &virtual_shelf("s1", "Fiction"), 0).unwrap();
        shelf_insert(&conn, &nested_shelf("s2", "Sci-fi", "s1"), 1).unwrap();
        shelf_insert(&conn, &nested_shelf("s3", "Space", "s2"), 2).unwrap();

        assert!(shelf_delete(&conn, "s2").unwrap());
        let back = shelves(&conn).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(
            back.iter().find(|s| s.id == "s3").unwrap().parent.as_deref(),
            Some("s1"),
            "s3 inherits the level s2 was on, not the top of the library"
        );

        // Removing a root shelf puts what was inside it at the root.
        assert!(shelf_delete(&conn, "s1").unwrap());
        let back = shelves(&conn).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "s3");
        assert_eq!(back[0].parent, None);
    }

    #[test]
    fn a_rename_is_refused_when_it_would_leave_the_shelf_nameless() {
        let conn = db();
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        assert!(shelf_rename(&conn, "s1", "Reading").unwrap());
        assert_eq!(shelves(&conn).unwrap()[0].name, "Reading");
        assert!(!shelf_rename(&conn, "s1", "   ").unwrap());
        assert_eq!(shelves(&conn).unwrap()[0].name, "Reading");
        assert!(!shelf_rename(&conn, "s1", "Reading").unwrap(), "the same name is not a change");
    }

    #[test]
    fn removing_a_shelf_keeps_its_books() {
        let conn = db();
        one_book(&conn);
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        shelf_add(&conn, "s1", "b1").unwrap();
        assert!(shelf_delete(&conn, "s1").unwrap());
        assert!(shelves(&conn).unwrap().is_empty());
        assert_eq!(book_count(&conn).unwrap(), 1, "a shelf holds ids, never books");
        assert!(!shelf_delete(&conn, "s1").unwrap());
    }

    #[test]
    fn a_move_lands_at_the_index_pointed_at() {
        let conn = db();
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        shelf_insert(&conn, &virtual_shelf("s2", "Other"), 1).unwrap();
        for index in 0..3u64 {
            let row = book(&format!("b{index}"), &format!("/books/{index}.pdf"), fp(index, index, index as u32));
            insert_book(&conn, &row, index as i64).unwrap();
            shelf_add(&conn, "s1", &format!("b{index}")).unwrap();
        }

        // Within one shelf: a drop before the first card.
        shelf_move(&conn, "b2", Some("s1"), "s1", Some(0)).unwrap();
        assert_eq!(shelves(&conn).unwrap()[0].books, vec!["b2", "b0", "b1"]);
        // Across shelves, appended.
        shelf_move(&conn, "b0", Some("s1"), "s2", None).unwrap();
        let back = shelves(&conn).unwrap();
        assert_eq!(back[0].books, vec!["b2", "b1"]);
        assert_eq!(back[1].books, vec!["b0"]);
        // Past the end clamps rather than panics.
        shelf_move(&conn, "b1", Some("s1"), "s2", Some(99)).unwrap();
        assert_eq!(shelves(&conn).unwrap()[1].books, vec!["b0", "b1"]);
    }

    // ------------------------------------------------------------------ folders --

    #[test]
    fn a_folder_ledger_round_trips() {
        let conn = db();
        let folder = WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts {
                min_size: 40 * 1024,
                watch: true,
                groups: false,
                ..FolderOpts::default()
            },
            placed: HashSet::from([fp(1, 2, 3)]),
            ignored: vec![Tombstone {
                fp: fp(4, 5, 6),
                title: Some("Removed".into()),
                format: Format::Markdown,
                last_path: "/books/gone.md".into(),
                shelf_id: Some("s1".into()),
                removed_ms: 77,
            }],
            shelf_map: BTreeMap::from([("scifi".to_string(), "s2".to_string())]),
            last_seen: vec![(fp(1, 2, 3), "/books/a.pdf".to_string())],
            scanned_ms: 900,
        };
        folder_upsert(&conn, &folder).unwrap();
        assert_eq!(folders(&conn).unwrap(), vec![folder.clone()]);

        // An upsert replaces the ledger rather than merging into it: the folder's
        // `placed` set is one aggregate, and a merge would be a second opinion
        // about which half of it is newer.
        let changed = WatchedFolder {
            placed: HashSet::new(),
            scanned_ms: 1000,
            ..folder
        };
        folder_upsert(&conn, &changed).unwrap();
        assert_eq!(folders(&conn).unwrap(), vec![changed]);
        assert!(folder_delete(&conn, "f1").unwrap());
        assert!(folders(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_ledger_names_the_folder_it_came_from() {
        let conn = db();
        conn.execute(
            "INSERT INTO folders (id, root, opts_json, placed_json, ignored_json, last_seen_json, shelf_map_json) \
             VALUES ('f1', '/books', 'not json', '[]', '[]', '[]', '{}')",
            [],
        )
        .unwrap();
        let error = folders(&conn).unwrap_err();
        assert!(error.contains("f1"), "{error}");
        assert!(error.contains("options"), "{error}");
    }

    // ------------------------------------------------------------------- covers --

    #[test]
    fn a_cover_round_trips_and_is_replaced_not_duplicated() {
        let conn = db();
        one_book(&conn);
        assert!(cover_get(&conn, "b1").unwrap().is_none());
        cover_put(&conn, "b1", &cover()).unwrap();
        assert_eq!(cover_get(&conn, "b1").unwrap(), Some(cover()));
        let smaller = Cover {
            width: 120.0,
            ..cover()
        };
        cover_put(&conn, "b1", &smaller).unwrap();
        assert_eq!(cover_get(&conn, "b1").unwrap(), Some(smaller));
    }

    #[test]
    fn a_cover_cannot_outlive_its_book() {
        // The foreign key, and not a cleanup pass somebody has to remember.
        let conn = db();
        one_book(&conn);
        cover_put(&conn, "b1", &cover()).unwrap();
        let error = cover_put(&conn, "nobody", &cover()).unwrap_err();
        assert!(error.contains("FOREIGN KEY"), "{error}");
    }

    // ----------------------------------------------------------------- highlights --

    #[test]
    fn saving_highlights_replaces_the_book_s_previous_set() {
        let conn = db();
        one_book(&conn);
        gloss_save(&conn, "b1", &[mark("m1", "palimpsest", "a palimpsest"), mark("m2", "gom", "a gom jabbar")]).unwrap();
        assert_eq!(gloss_load(&conn, "b1").unwrap().len(), 2);
        gloss_save(&conn, "b1", &[mark("m3", "kwisatz", "the kwisatz haderach")]).unwrap();
        let back = gloss_load(&conn, "b1").unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "m3");
    }

    #[test]
    fn an_answer_is_cached_against_its_mark_and_goes_with_it() {
        let conn = db();
        one_book(&conn);
        gloss_save(&conn, "b1", &[mark("m1", "palimpsest", "a palimpsest")]).unwrap();
        assert!(answer_get(&conn, "m1").unwrap().is_none());
        answer_put(&conn, "m1", "{\"definition\":\"a manuscript reused\"}", 12).unwrap();
        assert_eq!(
            answer_get(&conn, "m1").unwrap().as_deref(),
            Some("{\"definition\":\"a manuscript reused\"}")
        );
        answer_put(&conn, "m1", "{\"definition\":\"rewritten\"}", 13).unwrap();
        assert_eq!(
            answer_get(&conn, "m1").unwrap().as_deref(),
            Some("{\"definition\":\"rewritten\"}")
        );
        // The mark goes, the answer goes with it, and nothing has to sweep for it.
        gloss_save(&conn, "b1", &[]).unwrap();
        assert!(answer_get(&conn, "m1").unwrap().is_none());
    }

    // -------------------------------------------------------------------- search --

    #[test]
    fn search_finds_a_book_by_its_title_author_or_stem() {
        let conn = db();
        let dune = Book {
            title: Some("Dune".into()),
            author: Some("Frank Herbert".into()),
            ..book("b1", "/books/dune.pdf", fp(1, 1, 1))
        };
        let untitled = book("b2", "/scifi/Foundation Notes.md", fp(2, 2, 2));
        insert_book(&conn, &dune, 0).unwrap();
        insert_book(&conn, &untitled, 1).unwrap();

        assert_eq!(search(&conn, "dune", 10).unwrap().len(), 1);
        assert_eq!(search(&conn, "herbert", 10).unwrap().len(), 1);
        // The stem is what a book with no title of its own is reachable by.
        let by_stem = search(&conn, "foundation", 10).unwrap();
        assert_eq!(by_stem.len(), 1);
        assert!(search(&conn, "asimov", 10).unwrap().is_empty());
    }

    #[test]
    fn search_is_prefix_matched_case_blind_and_diacritic_blind() {
        let conn = db();
        let row = Book {
            title: Some("Résumé of Café Society".into()),
            ..book("b1", "/books/cafe.pdf", fp(1, 1, 1))
        };
        insert_book(&conn, &row, 0).unwrap();
        assert_eq!(search(&conn, "caf", 10).unwrap().len(), 1, "prefix");
        assert_eq!(search(&conn, "CAFE", 10).unwrap().len(), 1, "case and accents");
        assert_eq!(search(&conn, "resume", 10).unwrap().len(), 1, "diacritics");
        // Every term has to be found, in any order.
        assert_eq!(search(&conn, "society cafe", 10).unwrap().len(), 1);
        assert!(search(&conn, "cafe asimov", 10).unwrap().is_empty());
    }

    #[test]
    fn a_blank_or_punctuation_query_matches_nothing_and_errors_on_nothing() {
        let conn = db();
        one_book(&conn);
        for query in ["", "   ", "-", "\"", "\"\"", "...", "—"] {
            assert!(
                search(&conn, query, 10).unwrap().is_empty(),
                "{query:?} must answer with nothing rather than a syntax error"
            );
        }
    }

    #[test]
    fn search_excludes_a_book_whose_address_died() {
        let conn = db();
        one_book(&conn);
        assert_eq!(search(&conn, "dune", 10).unwrap().len(), 1);
        set_missing(&conn, "b1", true).unwrap();
        assert!(
            search(&conn, "dune", 10).unwrap().is_empty(),
            "a missing book is not a search result, it is a relink"
        );
    }

    #[test]
    fn the_index_follows_a_rename_and_a_removal() {
        // A book whose stem does not contain the word, so the title is the only
        // thing the index can find it by and the assertion means something.
        let conn = db();
        let row = Book {
            title: Some("Dune".into()),
            ..book("b1", "/books/x.pdf", fp(1, 1, 1))
        };
        insert_book(&conn, &row, 0).unwrap();
        assert_eq!(search(&conn, "dune", 10).unwrap().len(), 1);

        conn.execute("UPDATE books SET title = 'Arrakis' WHERE id = 'b1'", [])
            .unwrap();
        assert!(
            search(&conn, "dune", 10).unwrap().is_empty(),
            "the update trigger must drop the old title, or the index keeps selling a name the book no longer has"
        );
        assert_eq!(search(&conn, "arrakis", 10).unwrap().len(), 1);

        purge(&conn, "b1").unwrap();
        assert!(
            search(&conn, "arrakis", 10).unwrap().is_empty(),
            "the delete trigger fired rather than leaving a row the join cannot resolve"
        );
    }

    #[test]
    fn the_limit_is_honoured() {
        let conn = db();
        for index in 0..5u64 {
            let row = Book {
                title: Some(format!("Volume {index}")),
                ..book(
                    &format!("b{index}"),
                    &format!("/books/{index}.pdf"),
                    fp(index, index, index as u32),
                )
            };
            insert_book(&conn, &row, index as i64).unwrap();
        }
        assert_eq!(search(&conn, "volume", 2).unwrap().len(), 2);
        assert_eq!(search(&conn, "volume", 50).unwrap().len(), 5);
    }

    #[test]
    fn a_query_is_quoted_so_a_reader_cannot_write_fts_syntax() {
        assert_eq!(fts_query("rus servers"), "\"rus\"* \"servers\"*");
        assert_eq!(fts_query("a\"b"), "\"ab\"*");
        // FTS5's own operators arrive as ordinary text inside a quoted token, so a
        // reader typing NEAR(a b) searches for those characters rather than asking
        // the index for a proximity query.
        assert_eq!(fts_query("NEAR(a b)"), "\"NEAR(a\"* \"b)\"*");
        assert_eq!(fts_query("-"), "");
        assert_eq!(fts_query("  dune  "), "\"dune\"*");
    }

    // ------------------------------------------------------------- the small ones --

    #[test]
    fn the_format_column_names_are_the_registry_s_serde_names() {
        // The shell does not name reader-core, so the column spelling is written
        // down twice; this is the test that keeps the two from drifting, in the
        // same shape as tools/check-formats.ts keeps the extension lists honest.
        for format in [Format::Pdf, Format::Text, Format::Markdown] {
            let name = format_name(format);
            assert_eq!(
                serde_json::to_string(&format).unwrap(),
                format!("\"{name}\""),
                "{name} is not the serde name"
            );
            assert_eq!(format_of_name(name), format);
        }
        assert_eq!(format_of_name("epub"), Format::Pdf, "an unknown column reads as the default");
    }

    #[test]
    fn the_stem_column_is_the_fallback_a_search_needs() {
        assert_eq!(stem_of("/books/Dune.pdf"), "Dune");
        assert_eq!(stem_of("C:\\books\\Dune.PDF"), "Dune");
        // The stem rule is reader-core's, and it strips every extension the
        // format registry opens plus the office and print kinds, because a shelf
        // that said "notes.markdown" would be reading the file system's business
        // out loud. A title like "Rust 1.75" has to keep its ".75", and a name
        // with no extension at all keeps its own name.
        assert_eq!(stem_of("/books/Makefile"), "Makefile");
        assert_eq!(stem_of("/books/notes.markdown"), "notes");
        assert_eq!(stem_of("/books/log.txt"), "log");
        assert_eq!(stem_of("/books/Rust 1.75"), "Rust 1.75");
        assert_eq!(stem_of("/"), "/");
    }

    #[test]
    fn a_key_value_pair_round_trips() {
        let conn = db();
        assert_eq!(kv_get(&conn, "legacy_migrated").unwrap(), None);
        kv_set(&conn, "legacy_migrated", "{\"books\":3}").unwrap();
        assert_eq!(kv_get(&conn, "legacy_migrated").unwrap().as_deref(), Some("{\"books\":3}"));
        kv_set(&conn, "legacy_migrated", "{\"books\":4}").unwrap();
        assert_eq!(kv_get(&conn, "legacy_migrated").unwrap().as_deref(), Some("{\"books\":4}"));
    }

    #[test]
    fn an_empty_catalog_bootstraps_to_an_empty_library() {
        let conn = db();
        let snapshot = bootstrap(&conn).unwrap();
        assert!(snapshot.books.is_empty());
        assert!(snapshot.shelves.is_empty());
        assert!(snapshot.folders.is_empty());
    }

    #[test]
    fn a_bootstrap_reads_all_three_halves_in_one_go() {
        let conn = db();
        one_book(&conn);
        shelf_insert(&conn, &virtual_shelf("s1", "Mine"), 0).unwrap();
        shelf_add(&conn, "s1", "b1").unwrap();
        folder_upsert(&conn, &WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(1024, 7, 99)]),
            ignored: Vec::new(),
            shelf_map: BTreeMap::from([("".to_string(), "s1".to_string())]),
            last_seen: Vec::new(),
            scanned_ms: 1,
        })
        .unwrap();

        let snapshot = bootstrap(&conn).unwrap();
        assert_eq!(snapshot.books.len(), 1);
        assert_eq!(snapshot.shelves.len(), 1);
        assert_eq!(snapshot.shelves[0].books, vec!["b1".to_string()]);
        assert_eq!(snapshot.folders.len(), 1);
        assert_eq!(snapshot.folders[0].placed.len(), 1);
    }

    fn virtual_shelf(id: &str, name: &str) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: ShelfKind::Virtual,
            books: Vec::new(),
            parent: None,
        }
    }

    fn nested_shelf(id: &str, name: &str, parent: &str) -> Shelf {
        Shelf {
            parent: Some(parent.to_string()),
            ..virtual_shelf(id, name)
        }
    }
}
