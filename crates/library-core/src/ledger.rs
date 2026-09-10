//! The rescan ledger: what a scan of a watched folder DOES.
//!
//! This is the module the feature's edge cases live in. A rescan runs on every
//! window focus, so every case below happens repeatedly and silently — a wrong
//! answer is not a crash, it is a book that comes back after the reader
//! deleted it, or a library that gains a duplicate every time the window is
//! clicked. [`diff_folder`] is a pure function so each row of the decision
//! table is a test, and the tests below ARE the specification.
//!
//! The table, in full:
//!
//! | Scan finds a fingerprint…            | Book exists? | This folder placed it? | Removed from it? | Action   |
//! |--------------------------------------|--------------|------------------------|------------------|----------|
//! | not seen before                      | –            | –                      | –                | `Add`    |
//! | known, at the address already stored | yes          | yes                    | –                | `Skip`   |
//! | known, at a DIFFERENT address        | yes          | yes                    | –                | `Relink` |
//! | known, but the book is missing       | yes          | no                     | –                | `Relink` |
//! | known, placed by another folder      | yes          | no                     | –                | `Skip`   |
//! | seen here, but the row is gone       | no           | yes                    | –                | `Skip`   |
//! | anything                             | –            | –                      | yes              | `Skip`   |
//!
//! Row 3 is the "file moved on disk inside the watched tree" case: the address
//! is stale, the book is not, so the address is rewritten and every shelf
//! membership survives untouched. Row 4 is the same heal arriving from a
//! different folder than the one that placed the book — a book whose address
//! vanished is worth relinking whoever finds it. Row 5 is the same content in
//! two watched folders: the first one placed it, the second leaves it alone.
//! Row 6 is the reader having dragged a book off this folder's shelf (or the
//! row having been dropped by a storage trim): the fingerprint stays in
//! [`crate::folder::WatchedFolder::placed`], so the book is never re-added —
//! which is the "import back only the genuinely new ones" rule.
//!
//! The last row is the tombstone, and it is checked FIRST: a book the reader
//! deliberately removed is refused even when everything else about it says
//! "new".
//!
//! ## Two tables, because two questions are asked
//!
//! The table above is a RESCAN's answer — the passive walk a window focus
//! triggers, where "stay out" is the whole point of a tombstone and a
//! fingerprint this folder placed before is a book the reader filed away on
//! purpose. [`diff_import`] is an EXPLICIT import's answer, and two of its rows
//! lean the other way: a removal is an answer to "should this come back on its
//! own", not to "the reader is asking for it again". Import a folder you emptied
//! last week and the tombstones stand aside, because refusing them would make
//! "import this folder" silently refuse exactly the books the reader removed
//! from it — a broken import wearing a rule's clothes. Everything else is the
//! rescan's answer unchanged: a book the library holds at this address is a
//! Skip, at another address a Relink.

use std::collections::HashMap;

use crate::book::{Book, Fingerprint, Row, book_rows, book_rows_mut};
use crate::folder::{Tombstone, WatchedFolder};
use crate::scan::FoundFile;

/// What the ledger needs to know about a book that is already in the library:
/// its id (so a relink can write back to it), its address (so a move on disk
/// is recognisable as one) and whether that address is already known to be
/// dead. Everything else about the book is the reader's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBook {
    pub id: String,
    pub path: String,
    pub missing: bool,
}

/// Fingerprint → the library's row for it. Built from the book list on every
/// scan rather than persisted: it is a derived index, and a derived index
/// cannot go stale.
pub type Registry = HashMap<Fingerprint, KnownBook>;

/// Build the [`Registry`] for a row list, which is a walk of its BOOK rows: a
/// link has no fingerprint, so a scan cannot see one and never places one —
/// a pointer is not a copy of a file and a folder walk has nothing to say
/// about it. Two rows CAN share a fingerprint —
/// the duplicates a reader asked to keep (see [`crate::book::duplicate_title`])
/// — and the first row wins, which is [`crate::book::add_book`]'s own
/// resolution, so the index and an import agree about what "the book for this
/// content" names. For a scan the answer is the same whichever twin is named:
/// content the library holds is a Skip at its address and a Relink away from
/// it.
///
/// With one exception, and it is why this walks the list twice: a book of its
/// own ([`crate::book::Book::independent`]) is not the library's row for a
/// content, and a scan that named one would move the reader's private book
/// when the file moved on disk and leave the shared row — the one every other
/// layer answers with — pointing at a dead address. So the shared rows are
/// indexed first and a private row only answers for a content no shared row
/// holds, which keeps the alternative honest too: a file the library holds
/// ONLY as a private book is still held, and a scan that could not see it
/// would add a second row for a file already on the shelf.
pub fn registry_of(rows: &[Row]) -> Registry {
    let mut out = Registry::with_capacity(rows.len());
    let books: Vec<&Book> = book_rows(rows).collect();
    for book in books.iter().filter(|b| !b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    for book in books.iter().filter(|b| b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    out
}

/// What the ledger needs to know about one row.
fn known_of(book: &crate::book::Book) -> KnownBook {
    KnownBook {
        id: book.id.clone(),
        path: book.path().to_string(),
        missing: book.missing,
    }
}

/// One thing a scan decided to do about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanAction {
    /// A book the library does not have. The caller mints the row (linked or
    /// stored, per the folder's options) and records the fingerprint in
    /// [`crate::folder::WatchedFolder::placed`].
    Add(FoundFile),
    /// A known book whose address moved on disk. Rewrite the address; leave
    /// the id, the resume point and every shelf membership alone.
    Relink { book_id: String, to: String },
    /// Nothing to do. The common case by far — a rescan of an unchanged folder
    /// is a list of these.
    Skip,
}

impl ScanAction {
    /// True when the scan changes something. What the frontend uses to decide
    /// whether a rescan is worth a state write and a persist.
    pub fn is_change(&self) -> bool {
        !matches!(self, ScanAction::Skip)
    }
}

/// Decide what a folder's scan does, file by file, in the order the walk
/// produced it. Pure: it reads the folder's ledger and the global registry and
/// answers with one [`ScanAction`] per found file — it mutates neither, so the
/// caller can apply the whole batch or none of it.
pub fn diff_folder(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide(folder, registry, file));
    }
    out
}

/// One row of the decision table. Split out of [`diff_folder`] so a test can
/// name the case it is asserting instead of building a whole walk for it.
pub fn decide(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    // The tombstone wins over everything, including a fingerprint this folder
    // has never seen: the reader removed this file from the library, and it is
    // still on disk and still admitted by the folder's options.
    if folder.is_ignored(&file.fp) {
        return ScanAction::Skip;
    }
    match registry.get(&file.fp) {
        None => {
            // Unknown content. If this folder placed it before and the book
            // row is gone (removed by a storage trim, or hand-edited out of a
            // blob), do not resurrect it: the ledger remembers, the library
            // does not, and the reader's arrangement is the one that survives.
            if folder.placed.contains(&file.fp) {
                ScanAction::Skip
            } else {
                ScanAction::Add(file.clone())
            }
        }
        Some(known) => known_action(folder, known, file),
    }
}

/// The rows of both tables that answer the same way: a KNOWN fingerprint is a
/// skip at its own address and a relink when the address moved — moved inside
/// a tree this folder placed it in, or found by any folder while the book is
/// missing. What the two tables disagree about is the unknown and the removed,
/// which never reach here.
fn known_action(folder: &WatchedFolder, known: &KnownBook, file: &FoundFile) -> ScanAction {
    if known.path == file.path {
        return ScanAction::Skip;
    }
    // The address moved. This folder placed the book, so the move is
    // inside a tree it owns; or the book is already known to be
    // missing, in which case any watched tree that finds it heals it.
    if folder.placed.contains(&file.fp) || known.missing {
        ScanAction::Relink {
            book_id: known.id.clone(),
            to: file.path.clone(),
        }
    } else {
        ScanAction::Skip
    }
}

/// One row of an explicit import's table: the same questions as [`decide`],
/// with the two rows a previous removal owns answered the other way.
///
/// A tombstone is lifted rather than honoured, and a fingerprint this folder
/// placed before — whose book row is gone because the reader removed it — is an
/// Add rather than a Skip. The lift itself happens when the book actually lands
/// (see [`restore_deleted`]), not here: a copy that fails leaves the tombstone
/// standing, which is the one honest outcome for a file that could not be filed.
pub fn decide_import(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    match registry.get(&file.fp) {
        None => ScanAction::Add(file.clone()),
        Some(known) => known_action(folder, known, file),
    }
}

/// [`diff_folder`] for a run the reader asked for by name. See the module docs
/// for which two rows differ and why.
pub fn diff_import(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide_import(folder, registry, file));
    }
    out
}

/// Record a deliberate removal, so the next rescan stays quiet about the file.
///
/// Only the folders that PLACED the book take the tombstone: removing a book the
/// reader added by hand must not poison a watched folder that happens to contain
/// the same file, and removing a book one folder placed must not stop a second
/// folder from ever offering it. Both fall out of asking `placed` rather than
/// passing a folder id in from the UI.
pub fn tombstone(folders: &mut [WatchedFolder], entry: &Tombstone) {
    for folder in folders.iter_mut() {
        if folder.placed.contains(&entry.fp) && !folder.is_ignored(&entry.fp) {
            folder.ignored.push(entry.clone());
        }
    }
}

/// Drop the tombstones whose books came back.
///
/// Run inside every scan, before the diff: a fingerprint can rejoin the library
/// by any route — a hand-open, a second folder's import, a restore — and a
/// tombstone left behind for a book that exists is a restore row offering
/// something the reader already has.
pub fn prune_tombstones(folder: &mut WatchedFolder, registry: &Registry) {
    folder.ignored.retain(|entry| !registry.contains_key(&entry.fp));
}

/// The tombstone for `fp`, without taking it. A restore measures the file before
/// it promises anything, and a removal that stays put when the measurement fails
/// is the difference between "that file is gone" and a book quietly lost.
pub fn find_tombstone<'a>(folder: &'a WatchedFolder, fp: &Fingerprint) -> Option<&'a Tombstone> {
    folder.ignored.iter().find(|entry| &entry.fp == fp)
}

/// Take the tombstone for `fp` out of the folder and hand it back, so the caller
/// can put the book back.
///
/// Does NOT touch `placed`: the import that follows marks the placement when the
/// book actually lands, and marking it here would leave a fingerprint the ledger
/// skips with no book behind it — the one state that cannot be recovered from
/// without a rescan of the folder's options.
pub fn restore_deleted(folder: &mut WatchedFolder, fp: &Fingerprint) -> Option<Tombstone> {
    let at = folder.ignored.iter().position(|entry| &entry.fp == fp)?;
    Some(folder.ignored.remove(at))
}

/// Fingerprint to book, for the questions that start from a file rather than from
/// an address. Borrowed rather than cloned: the menu that asks these opens on a
/// click, and copying a whole library to answer one question about it is the kind
/// of cost that turns a click into a frame drop. Duplicates — the two rows a
/// reader asked to keep — resolve to the first, the rule [`registry_of`] gives.
pub fn index_by_fp(rows: &[Row]) -> HashMap<Fingerprint, &Book> {
    let mut out = HashMap::with_capacity(rows.len());
    for book in book_rows(rows) {
        out.entry(book.fp).or_insert(book);
    }
    out
}

/// A book this folder could give the reader back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// Removed by the reader, and still not in the library. The file may or may
    /// not still be on disk — a restore measures before it promises.
    Deleted(Tombstone),
    /// Still in the library, still inside this folder on disk, but no longer on
    /// any shelf this folder owns: the reader moved it somewhere else in the app.
    /// Nothing is wrong, and nothing is re-imported — the offer is to show it here
    /// as well, or to go and look at where it went.
    Moved {
        book_id: String,
        /// The document's own title, when it had one. `None` is common (a book
        /// imported and never opened), and the menu falls back to the file stem.
        title: Option<String>,
        path: String,
        /// The first shelf the book is on, by name, for the "now in Fiction" half
        /// of the row. `None` for a book that is in the library and on no shelf.
        home_shelf: Option<String>,
    },
}

/// What this folder's import menu can offer to give back.
///
/// Pure and synchronous, and that is the design: the menu opens on a click and
/// answers from the last scan's `last_seen` plus the folder's tombstones, so it
/// costs a walk over two short lists rather than a walk over a directory tree.
/// A restore re-measures the one file it is about to import, which is where the
/// freshness actually matters.
///
/// `membership` answers, for a book id, the shelves it is on as
/// `(id, name)` pairs in shelf order — the order is what makes "first membership"
/// a deterministic answer rather than whichever the map happened to yield.
pub fn recoverables(
    folder: &WatchedFolder,
    books_by_fp: &HashMap<Fingerprint, &Book>,
    membership: &impl Fn(&str) -> Vec<(String, String)>,
    folder_shelf_ids: &[String],
) -> Vec<Recovered> {
    let mut out = Vec::new();

    // Removed books first: a row that offers something back is worth more than a
    // row that offers to show you something you already have.
    for entry in &folder.ignored {
        // A book that came back by another route is not a recovery, and the next
        // scan's `prune_tombstones` will say so properly.
        if books_by_fp.contains_key(&entry.fp) {
            continue;
        }
        out.push(Recovered::Deleted(entry.clone()));
    }

    let on_a_folder_shelf = |shelves: &[(String, String)]| {
        shelves
            .iter()
            .any(|(id, _)| folder_shelf_ids.iter().any(|own| own == id))
    };
    for (fp, path) in &folder.last_seen {
        let Some(book) = books_by_fp.get(fp) else {
            continue;
        };
        // A book whose address died is a RELINK, and the card already offers one:
        // listing it here too would be a second door to the same room, and this
        // one would not know the address is bad.
        if book.missing {
            continue;
        }
        let shelves = membership(&book.id);
        if on_a_folder_shelf(&shelves) {
            continue;
        }
        out.push(Recovered::Moved {
            book_id: book.id.clone(),
            title: book.title.clone(),
            path: path.clone(),
            home_shelf: shelves.first().map(|(_, name)| name.clone()),
        });
    }
    out
}

/// Apply a `Relink` to a book list: rewrite the address, clear `missing`, and
/// keep everything else. Returns true when a row was written.
///
/// A linked book takes the new address outright. A stored book does NOT — its
/// bytes are the app's own copy, and a source file moving is provenance, not a
/// new address — so only its recorded source moves, and `missing` clears only
/// if the store copy is the thing that was checked.
pub fn relink(rows: &mut [Row], book_id: &str, to: &str) -> bool {
    // A link is never relinked and never the row a relink names: it has no
    // address to move, and the scan that asked for this walk could not have
    // seen it.
    let Some(book) = book_rows_mut(rows).find(|b| b.id == book_id) else {
        return false;
    };
    match &mut book.origin {
        crate::book::Origin::Linked { src } => {
            *src = to.to_string();
            book.missing = false;
        }
        crate::book::Origin::Stored { src, .. } => {
            *src = Some(to.to_string());
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Origin};
    use crate::folder::{FolderOpts, Tombstone};
    use reader_core::format::Format;
    use std::collections::{BTreeMap, HashSet};

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn file(n: u32, path: &str) -> FoundFile {
        FoundFile {
            path: path.to_string(),
            rel: path.trim_start_matches("/books/").to_string(),
            ext: "pdf".into(),
            size: u64::from(n),
            fp: fp(n),
        }
    }

    fn folder(placed: &[u32], ignored: &[u32]) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: placed.iter().copied().map(fp).collect::<HashSet<_>>(),
            ignored: ignored.iter().copied().map(stone).collect(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        }
    }

    fn stone(n: u32) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some(format!("Book {n}")),
            format: reader_core::format::Format::Pdf,
            last_path: format!("/books/{n}.pdf"),
            shelf_id: None,
            removed_ms: 5,
        }
    }

    fn registry(rows: &[(u32, &str, &str, bool)]) -> Registry {
        rows.iter()
            .map(|(n, id, path, missing)| {
                (
                    fp(*n),
                    KnownBook {
                        id: (*id).to_string(),
                        path: (*path).to_string(),
                        missing: *missing,
                    },
                )
            })
            .collect()
    }

    /// Row 1: content the library has never seen is added.
    #[test]
    fn an_unknown_fingerprint_is_added() {
        let f = folder(&[], &[]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Add(file(1, "/books/a.pdf")));
    }

    /// Row 2: nothing moved, nothing to do.
    #[test]
    fn a_known_book_at_its_own_address_is_skipped() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    /// Row 3: the file moved inside the watched tree.
    #[test]
    fn a_known_book_at_a_new_address_is_relinked() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(
            decide(&f, &r, &file(1, "/books/moved/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved/a.pdf".into()
            }
        );
    }

    /// Row 4: a book whose address died is healed by whichever folder finds it.
    #[test]
    fn a_missing_book_is_relinked_by_a_folder_that_never_placed_it() {
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/gone/a.pdf", true)]);
        assert!(matches!(
            decide(&f, &r, &file(1, "/books/a.pdf")),
            ScanAction::Relink { .. }
        ));
    }

    /// Row 5: the same content in two watched folders belongs to the first.
    #[test]
    fn a_second_folder_never_relinks_a_book_it_did_not_place() {
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    /// Row 6 — the rule the whole ledger exists for: a book the reader moved
    /// off this folder's shelf is still in the library, and a rescan must not
    /// put it back or add a second copy.
    #[test]
    fn a_book_moved_off_the_folder_shelf_is_never_re_added() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        // And with the row gone but the ledger remembering: still no add.
        let f = folder(&[1], &[]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    /// Row 7: the tombstone outranks every other row.
    #[test]
    fn a_removed_book_stays_removed() {
        let f = folder(&[], &[1]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
        // Even when the file itself moved.
        let f = folder(&[1], &[1]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/elsewhere.pdf")), ScanAction::Skip);
    }

    /// The import table: a removal is an answer to "should this come back on
    /// its own", not to "the reader is asking for it again". Emptying a watched
    /// folder's shelf and then importing the folder again must give the books
    /// back, or the import reads as broken rather than as a choice.
    #[test]
    fn an_explicit_import_overrides_the_removals_that_wrote_the_tombstones() {
        let f = folder(&[], &[1]);
        assert_eq!(
            decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
        // Row 6 leans the same way: the ledger remembering a book whose row is
        // gone is a rescan's reason to stay quiet, not an import's.
        let f = folder(&[1], &[]);
        assert_eq!(
            decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
    }

    /// The import table keeps every row the tombstone does not own: a book the
    /// library already holds at this address is still a Skip, and at another
    /// address still a Relink — an explicit import is not a licence to
    /// duplicate what the reader has.
    #[test]
    fn an_explicit_import_still_refuses_to_duplicate_a_book_it_has() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide_import(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        assert_eq!(
            decide_import(&f, &r, &file(1, "/books/moved/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved/a.pdf".into()
            }
        );
        // And a book another folder placed is still none of this folder's business.
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
        assert_eq!(decide_import(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    #[test]
    fn a_whole_walk_answers_in_order_and_only_changes_are_changes() {
        let f = folder(&[2], &[3]);
        let r = registry(&[(2, "b2", "/books/b.pdf", false)]);
        let walk = vec![
            file(1, "/books/a.pdf"),
            file(2, "/books/b.pdf"),
            file(3, "/books/c.pdf"),
            file(4, "/books/sub/d.pdf"),
        ];
        let actions = diff_folder(&f, &r, &walk);
        assert_eq!(actions.len(), walk.len());
        assert!(actions[0].is_change());
        assert!(!actions[1].is_change());
        assert!(!actions[2].is_change());
        assert!(actions[3].is_change());
        assert_eq!(actions.iter().filter(|a| a.is_change()).count(), 2);
    }

    #[test]
    fn an_unchanged_folder_costs_one_state_write_nothing() {
        // The common case: a focus rescan of a folder nobody touched. Every
        // answer is Skip, so the frontend can skip the write and the persist.
        let f = folder(&[1, 2], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false), (2, "b2", "/books/b.pdf", false)]);
        let walk = vec![file(1, "/books/a.pdf"), file(2, "/books/b.pdf")];
        assert!(diff_folder(&f, &r, &walk).iter().all(|a| !a.is_change()));
    }

    #[test]
    fn only_the_folders_that_placed_a_book_take_its_tombstone() {
        let mut folders = vec![folder(&[1], &[]), folder(&[2], &[]), folder(&[], &[])];
        folders[1].id = "f2".into();
        folders[2].id = "f3".into();
        tombstone(&mut folders, &stone(1));
        assert!(folders[0].is_ignored(&fp(1)));
        assert!(folders[1].ignored.is_empty());
        assert!(folders[2].ignored.is_empty());
        // A book no folder placed (added by hand) poisons nothing.
        tombstone(&mut folders, &stone(9));
        assert!(folders.iter().all(|f| f.ignored.len() <= 1));
    }

    #[test]
    fn placing_a_file_is_what_makes_the_next_scan_skip_it() {
        let mut f = folder(&[], &[]);
        f.mark_placed(fp(1));
        // The book row is not in the registry yet (the frontend adds it in the
        // same batch), so `placed` alone is what stops a second add.
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    /// A book row — the library's list holds rows, and every rule here reads
    /// the books among them.
    fn book(id: &str, origin: Origin, missing: bool) -> Row {
        Row::Book(book_value(id, origin, missing))
    }

    fn book_value(id: &str, origin: Origin, missing: bool) -> Book {
        Book {
            id: id.to_string(),
            fp: fp(1),
            title: Some("Dune".into()),
            author: None,
            format: Format::Pdf,
            origin,
            added_ms: 1,
            last_read_ms: 5,
            page: 42,
            num_pages: 400,
            fraction: None,
            missing,
            fp_pending: false,
            independent: false,
        }
    }

    /// The book a row holds. Every row these tests build is a book.
    fn at(rows: &[Row], i: usize) -> &Book {
        rows[i].book().expect("a book row")
    }

    #[test]
    fn a_relink_moves_the_address_and_keeps_everything_else() {
        let mut books = vec![book("b1", Origin::Linked { src: "/gone/a.pdf".into() }, true)];
        assert!(relink(&mut books, "b1", "/books/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/books/a.pdf");
        assert!(!at(&books, 0).missing);
        assert_eq!(at(&books, 0).page, 42, "the resume point is the reader's, not the scan's");
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(books[0].id(), "b1");
        assert!(!relink(&mut books, "zzz", "/x"));
    }

    #[test]
    fn a_relink_never_repoints_a_stored_book_at_the_source() {
        // The store copy is what the reader opens; a source file moving is
        // provenance. Repointing it would make the app's own copy unreachable
        // and the "survives the source folder being deleted" promise false.
        let mut books = vec![book(
            "b1",
            Origin::Stored {
                src: Some("/gone/a.pdf".into()),
                store: "/app/store/pdf/a_b1.pdf".into(),
            },
            false,
        )];
        assert!(relink(&mut books, "b1", "/downloads/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/app/store/pdf/a_b1.pdf");
        assert_eq!(at(&books, 0).origin.source(), Some("/downloads/a.pdf"));
    }


    /// A book with its own fingerprint, so a test can hold two of them.
    fn sized_book(id: &str, n: u32, path: &str, missing: bool) -> Row {
        Row::Book(Book {
            fp: fp(n),
            origin: Origin::Linked {
                src: path.to_string(),
            },
            missing,
            ..book_value(id, Origin::Linked { src: path.to_string() }, false)
        })
    }

    const NO_SHELVES: fn(&str) -> Vec<(String, String)> = |_| Vec::new();

    #[test]
    fn a_removed_book_is_offered_back_with_enough_to_recognise_it() {
        let f = folder(&[1], &[2]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &NO_SHELVES, &["s1".to_string()]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Recovered::Deleted(entry) => {
                assert_eq!(entry.fp, fp(2));
                assert_eq!(entry.label(), "Book 2");
                assert_eq!(entry.last_path, "/books/2.pdf");
            }
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_book_that_came_back_by_another_route_is_not_a_recovery() {
        // The next scan's prune says so properly; the menu must not offer a book
        // the reader already has.
        let f = folder(&[1], &[1]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &NO_SHELVES, &[]).is_empty());
    }

    #[test]
    fn pruning_drops_the_tombstone_of_a_book_that_returned() {
        // Pruning runs inside a scan, so it reads the scan's own registry rather
        // than a second index built for the menu.
        let mut f = folder(&[1], &[1, 2]);
        let reg = registry(&[(1, "b1", "/books/1.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        let left: Vec<Fingerprint> = f.ignored.iter().map(|t| t.fp).collect();
        assert_eq!(left, vec![fp(2)], "only the book that is really gone stays");
    }

    #[test]
    fn a_book_moved_off_every_folder_shelf_is_offered_as_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let elsewhere = |_: &str| vec![("s9".to_string(), "Fiction".to_string())];
        assert_eq!(
            recoverables(&f, &index, &elsewhere, &["s1".to_string()]),
            vec![Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/1.pdf".into(),
                home_shelf: Some("Fiction".into()),
            }]
        );
    }

    #[test]
    fn a_book_still_on_one_of_the_folders_shelves_is_not_a_move() {
        // The rule that keeps "also show it here" from ever double-placing.
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let here = |_: &str| vec![("s1".to_string(), "Books".to_string())];
        assert!(recoverables(&f, &index, &here, &["s1".to_string()]).is_empty());
    }

    #[test]
    fn a_book_on_a_shelf_the_folder_does_not_own_is_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let elsewhere = |_: &str| vec![("s9".to_string(), "Fiction".to_string())];
        let out = recoverables(&f, &index, &elsewhere, &["s1".to_string(), "s2".to_string()]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_missing_book_is_a_relink_and_not_a_move() {
        // The card already offers a relink; a second door to the same room would
        // be one that does not know the address is bad.
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", true)];
        let index = index_by_fp(&books);
        let elsewhere = |_: &str| vec![("s9".to_string(), "Fiction".to_string())];
        assert!(recoverables(&f, &index, &elsewhere, &["s1".to_string()]).is_empty());
    }

    #[test]
    fn a_file_no_longer_in_the_tree_is_neither_a_move_nor_a_removal() {
        let mut f = folder(&[1, 2], &[]);
        // The last scan saw only fp(2); fp(1) has left the folder on disk.
        f.last_seen = vec![(fp(2), "/books/2.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &NO_SHELVES, &["s1".to_string()]).is_empty());
    }

    #[test]
    fn removals_are_listed_before_moves() {
        let mut f = folder(&[1, 2], &[2]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let elsewhere = |_: &str| vec![("s9".to_string(), "Fiction".to_string())];
        let out = recoverables(&f, &index, &elsewhere, &["s1".to_string()]);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], Recovered::Deleted(_)));
        assert!(matches!(out[1], Recovered::Moved { .. }));
    }

    #[test]
    fn a_home_shelf_is_the_first_one_in_shelf_order() {
        // Deterministic rather than whichever a map happened to yield, because
        // the row's label is a sentence and a sentence cannot change per open.
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let two = |_: &str| {
            vec![
                ("s8".to_string(), "Fiction".to_string()),
                ("s3".to_string(), "Classics".to_string()),
            ]
        };
        let out = recoverables(&f, &index, &two, &["s1".to_string()]);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => {
                assert_eq!(home_shelf.as_deref(), Some("Fiction"))
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_book_on_no_shelf_at_all_has_no_home_to_name() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &NO_SHELVES, &["s1".to_string()]);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => assert_eq!(home_shelf, &None),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_restore_takes_the_tombstone_and_leaves_the_placement_to_the_import() {
        let mut f = folder(&[1], &[2]);
        assert!(find_tombstone(&f, &fp(2)).is_some());
        assert!(find_tombstone(&f, &fp(9)).is_none());
        // Peeking must not consume: a restore measures the file before it
        // promises anything, and a removal that stays put when the measurement
        // fails is the difference between "that file is gone" and a lost book.
        assert!(find_tombstone(&f, &fp(2)).is_some());
        let taken = restore_deleted(&mut f, &fp(2)).expect("present");
        assert_eq!(taken.fp, fp(2));
        assert!(!f.is_ignored(&fp(2)));
        assert!(
            !f.placed.contains(&fp(2)),
            "the import marks the placement, not the restore"
        );
        assert!(restore_deleted(&mut f, &fp(2)).is_none());
    }

    #[test]
    fn a_tombstone_is_the_record_a_restore_row_needs() {
        let b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/dune.pdf".into(),
            },
            false,
        );
        let entry = Tombstone::of(&b, Some("s2".into()), 999);
        assert_eq!(entry.fp, b.fp);
        assert_eq!(entry.title.as_deref(), Some("Dune"));
        assert_eq!(entry.format, reader_core::format::Format::Pdf);
        assert_eq!(entry.last_path, "/books/dune.pdf");
        assert_eq!(entry.shelf_id.as_deref(), Some("s2"));
        assert_eq!(entry.removed_ms, 999);
        assert_eq!(entry.label(), "Dune");
    }

    #[test]
    fn a_tombstone_of_a_book_never_opened_labels_itself_from_the_file() {
        let mut b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/rust-book.pdf".into(),
            },
            false,
        );
        b.title = None;
        assert_eq!(Tombstone::of(&b, None, 1).label(), "rust-book");
    }

    #[test]
    fn a_tombstone_crosses_the_wire_with_its_camel_case_names() {
        let entry = stone(3);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"lastPath\""), "{json}");
        assert!(json.contains("\"removedMs\""), "{json}");
        assert!(json.contains("\"shelfId\""), "{json}");
        assert!(!json.contains('_'), "{json}");
        let back: Tombstone = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
        // A blob from before the shelf id existed still loads.
        let older: Tombstone = serde_json::from_str(
            r#"{"fp":{"size":1,"mtimeMs":1,"headHash":1},"format":"pdf",
                "lastPath":"/a.pdf","removedMs":2}"#,
        )
        .unwrap();
        assert_eq!(older.shelf_id, None);
        assert_eq!(older.title, None);
    }

    #[test]
    fn the_last_scan_is_remembered_only_for_books_this_folder_placed() {
        let mut f = folder(&[1], &[]);
        let found = vec![file(1, "/books/1.pdf"), file(2, "/books/2.pdf")];
        f.record_seen(&found);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/1.pdf".to_string())]);
        // A second scan REPLACES the first rather than adding to it: the menu
        // answers "where is it now", not "where has it ever been".
        f.record_seen(&[]);
        assert!(f.last_seen.is_empty());
    }

    #[test]
    fn a_scan_that_changed_nothing_still_refreshes_what_was_seen() {
        // The common case, and the reason `record_seen` is not behind the
        // "did anything change" check: a book moved out of the folder between two
        // quiet scans is exactly what the menu has to be able to see.
        let mut f = folder(&[1], &[]);
        f.record_seen(&[file(1, "/books/1.pdf")]);
        f.record_seen(&[file(1, "/books/moved/1.pdf")]);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/moved/1.pdf".to_string())]);
    }

    #[test]
    fn a_link_is_invisible_to_a_scan() {
        // A pointer at a book is not a copy of a file: it has no fingerprint
        // for a walk to match, no address to relink and nothing a folder could
        // place. A registry that could see one would answer Relink for a row
        // that has no address to move.
        let rows = vec![
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
        ];
        let r = registry_of(&rows);
        assert_eq!(r.len(), 1, "the link is not in it");
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(index_by_fp(&rows).len(), 1);
        // And a relink asked for a link's id is a relink that finds no book.
        let mut rows = rows;
        assert!(!relink(&mut rows, "l1", "/somewhere/a.pdf"));
        assert!(relink(&mut rows, "b1", "/moved/a.pdf"));
        assert_eq!(at(&rows, 1).path(), "/moved/a.pdf");
    }

    #[test]
    fn the_registry_is_built_from_the_books_and_first_row_wins() {
        let books = vec![
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
            Row::Book(Book {
                id: "dup".into(),
                ..book_value("b1", Origin::Linked { src: "/books/a.pdf".into() }, false)
            }),
        ];
        let r = registry_of(&books);
        assert_eq!(r.len(), 1);
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(r[&fp(1)].path, "/books/a.pdf");
        assert!(!r[&fp(1)].missing);
    }
}
