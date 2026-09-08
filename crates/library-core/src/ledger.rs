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

use std::collections::HashMap;

use crate::book::Fingerprint;
use crate::folder::WatchedFolder;
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

/// Build the [`Registry`] for a book list. Two books sharing a fingerprint
/// cannot happen after [`crate::book::sanitize`], but a blob that predates it
/// could carry one, and the first row wins — the same rule the sanitizer
/// applies, so the index and the list agree.
pub fn registry_of(books: &[crate::book::Book]) -> Registry {
    let mut out = Registry::with_capacity(books.len());
    for book in books {
        out.entry(book.fp).or_insert_with(|| KnownBook {
            id: book.id.clone(),
            path: book.path().to_string(),
            missing: book.missing,
        });
    }
    out
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
    if folder.ignored.contains(&file.fp) {
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
        Some(known) => {
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
    }
}

/// Record a deliberate removal, so the next rescan stays quiet about the file.
///
/// Only the folders that PLACED the book take the tombstone: removing a book
/// the reader added by hand must not poison a watched folder that happens to
/// contain the same file, and removing a book one folder placed must not stop
/// a second folder from ever offering it. Both fall out of asking `placed`
/// rather than passing a folder id in from the UI.
pub fn tombstone(folders: &mut [WatchedFolder], fp: Fingerprint) {
    for folder in folders.iter_mut() {
        if folder.placed.contains(&fp) {
            folder.ignored.insert(fp);
        }
    }
}

/// Apply an `Add` to the folder's ledger. One call per placed file, so the
/// ledger and the book list are written by the same code path that read them.
pub fn mark_placed(folder: &mut WatchedFolder, file: &FoundFile) {
    folder.mark_placed(file.fp);
}

/// Apply a `Relink` to a book list: rewrite the address, clear `missing`, and
/// keep everything else. Returns true when a row was written.
///
/// A linked book takes the new address outright. A stored book does NOT — its
/// bytes are the app's own copy, and a source file moving is provenance, not a
/// new address — so only its recorded source moves, and `missing` clears only
/// if the store copy is the thing that was checked.
pub fn relink(books: &mut [crate::book::Book], book_id: &str, to: &str) -> bool {
    let Some(book) = books.iter_mut().find(|b| b.id == book_id) else {
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
    use crate::folder::FolderOpts;
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
            ignored: ignored.iter().copied().map(fp).collect::<HashSet<_>>(),
            shelf_map: BTreeMap::new(),
            scanned_ms: 0,
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
        tombstone(&mut folders, fp(1));
        assert!(folders[0].ignored.contains(&fp(1)));
        assert!(folders[1].ignored.is_empty());
        assert!(folders[2].ignored.is_empty());
        // A book no folder placed (added by hand) poisons nothing.
        tombstone(&mut folders, fp(9));
        assert!(folders.iter().all(|f| f.ignored.len() <= 1));
    }

    #[test]
    fn placing_a_file_is_what_makes_the_next_scan_skip_it() {
        let mut f = folder(&[], &[]);
        mark_placed(&mut f, &file(1, "/books/a.pdf"));
        // The book row is not in the registry yet (the frontend adds it in the
        // same batch), so `placed` alone is what stops a second add.
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    fn book(id: &str, origin: Origin, missing: bool) -> Book {
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
        }
    }

    #[test]
    fn a_relink_moves_the_address_and_keeps_everything_else() {
        let mut books = vec![book("b1", Origin::Linked { src: "/gone/a.pdf".into() }, true)];
        assert!(relink(&mut books, "b1", "/books/a.pdf"));
        assert_eq!(books[0].path(), "/books/a.pdf");
        assert!(!books[0].missing);
        assert_eq!(books[0].page, 42, "the resume point is the reader's, not the scan's");
        assert_eq!(books[0].title.as_deref(), Some("Dune"));
        assert_eq!(books[0].id, "b1");
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
        assert_eq!(books[0].path(), "/app/store/pdf/a_b1.pdf");
        assert_eq!(books[0].origin.source(), Some("/downloads/a.pdf"));
    }

    #[test]
    fn the_registry_is_built_from_the_books_and_first_row_wins() {
        let books = vec![
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
            Book {
                id: "dup".into(),
                ..book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false)
            },
        ];
        let r = registry_of(&books);
        assert_eq!(r.len(), 1);
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(r[&fp(1)].path, "/books/a.pdf");
        assert!(!r[&fp(1)].missing);
    }
}
