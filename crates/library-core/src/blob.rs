//! The persisted shape of the library, and the migration from the shape it
//! replaced.
//!
//! One key holds the whole thing — books, shelves, watched folders and the
//! view — because they are one invariant: a shelf member that names no book is
//! a hole in the grid, and a folder ledger that remembers a fingerprint no
//! book carries is a book that can never come back. Loading them together is
//! what lets [`sanitize`] enforce that with both halves in hand.
//!
//! The covers are NOT here. They stayed in their own key
//! (`pdfreader.covers.v1`, written by `src/storage/mod.rs`) through this schema
//! change on purpose: a cover is a base64 JPEG, tens of kilobytes each, and a
//! library write happens on every page turn. Putting the images in the same
//! blob would make turning a page re-serialise the whole shelf's art.

use serde::{Deserialize, Serialize};

use crate::book::{Book, Fingerprint, Origin};
use crate::folder::WatchedFolder;
use crate::shelf::{Shelf, ShelfKind};
use crate::view::LibraryView;

/// The library's localStorage key. `v2` rather than a schema edit under `v1`:
/// the row shape changed (a path became an id, an origin and a fingerprint),
/// and a blob this build cannot parse must not be overwritten by the default
/// before [`migrate_v1`] has had a look at it.
pub const LIBRARY_KEY: &str = "pdfreader.library.v2";

/// The key the previous schema lived under. Read once, on a load that finds no
/// `v2`, and left in place afterwards — a downgrade should still see the
/// library it wrote.
pub const LEGACY_KEY: &str = "pdfreader.library.v1";

/// The whole library, as persisted.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBlob {
    /// Every book, in the order the "All" shelf shows them. This list IS the
    /// All order — there is no separate shelf for it (see [`crate::shelf`]).
    #[serde(default)]
    pub books: Vec<Book>,
    #[serde(default)]
    pub shelves: Vec<Shelf>,
    #[serde(default)]
    pub folders: Vec<WatchedFolder>,
    #[serde(default)]
    pub view: LibraryView,
}

impl LibraryBlob {
    /// Whether the blob holds any books at all.
    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// True when some book still carries a placeholder fingerprint, i.e. the
    /// library has not been checked against the filesystem since it loaded.
    /// The frontend holds a folder rescan until this clears: scanning against
    /// unmeasured fingerprints would add a second copy of every migrated book.
    pub fn awaiting_check(&self) -> bool {
        self.books.iter().any(|b| b.fp_pending)
    }
}

/// The `v1` row: a path, a title and a resume point. Kept in this crate (not
/// in the app) because the migration is a rule about the library's shape, and
/// a rule is something a test can call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBook {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    #[serde(default)]
    pub fraction: Option<f64>,
}

fn default_page() -> u32 {
    1
}

/// Turn a `v1` recent-books list into a library.
///
/// Every row becomes a [`Origin::Linked`] book — read in place was the only
/// mode the old build had, and a migration that quietly copied two gigabytes
/// of PDFs into an app store would be the worst possible surprise. The order
/// survives, so the shelf the reader had is the shelf they get.
///
/// A `v1` row carries no measurement, so its fingerprint is a placeholder
/// derived from the address ([`Fingerprint::placeholder`]) and the book is
/// marked [`Book::fp_pending`]. The first path check replaces it with the real
/// one; until then [`LibraryBlob::awaiting_check`] holds a rescan off, because
/// a scan comparing real fingerprints against placeholders would add a second
/// copy of every book already on the shelf.
pub fn migrate_v1(legacy: Vec<RecentBook>, now_ms: u64) -> LibraryBlob {
    let books = legacy
        .into_iter()
        .filter(|b| !b.path.trim().is_empty())
        .enumerate()
        .map(|(i, b)| {
            let id = crate::id::new_id(now_ms, i as u32);
            Book {
                fp: Fingerprint::placeholder(&b.path),
                format: reader_core::format::format_of(&b.path),
                origin: Origin::Linked { src: b.path },
                title: b.title.filter(|t| !t.trim().is_empty()),
                author: None,
                id,
                // A migrated book was opened, so it has been read — but the old
                // schema kept no stamp, and `now_ms` would put every book at
                // the top of a "Last read" sort. Zero sorts them together,
                // below anything read since, which is the honest answer.
                added_ms: 0,
                last_read_ms: 0,
                page: b.page.max(1),
                num_pages: b.num_pages,
                fraction: b.fraction.filter(|f| (0.0..=1.0).contains(f)),
                missing: false,
                fp_pending: true,
            }
        })
        .collect();
    LibraryBlob {
        books,
        shelves: Vec::new(),
        folders: Vec::new(),
        view: LibraryView::default(),
    }
}

/// Make a persisted library internally valid, with every half in hand:
/// the per-list rules ([`crate::book::sanitize`], [`crate::folder::sanitize`],
/// [`crate::view::sanitize`], [`crate::shelf::sanitize`]) plus the two that
/// only make sense across lists — a shelf member that names no book, and a
/// folder shelf whose folder is gone. Idempotent.
///
/// The shelves go LAST, and the order is the point. A folder shelf whose folder
/// is gone is dropped here, and a shelf nested inside it is then pointing at a
/// parent that no longer exists — which is [`crate::shelf::sanitize`]'s to
/// collapse back to the root. Sanitising the shelves first would leave that
/// dangling parent in place, and the shelf it belongs to would render on no
/// level at all: a folder the reader can never open again.
pub fn sanitize(blob: &mut LibraryBlob) {
    crate::book::sanitize(&mut blob.books);
    crate::folder::sanitize(&mut blob.folders);
    crate::view::sanitize(&mut blob.view);

    // A blob could carry two rows with one id however valid its books look;
    // `book::sanitize` dedupes by id, so by here the shelves below resolve
    // against a list whose ids are unique — and whose fingerprints are NOT,
    // because a duplicate the reader kept is two honest rows of one file.
    let known: std::collections::HashSet<&str> =
        blob.books.iter().map(|b| b.id.as_str()).collect();
    for shelf in blob.shelves.iter_mut() {
        shelf.books.retain(|m| known.contains(m.as_str()));
    }

    // A folder shelf whose folder was removed has no rescan to refill it and
    // no watch dot to explain it; it is a stale tile, so it goes. A virtual
    // shelf is the reader's own and is never dropped for this reason.
    let folders: std::collections::HashSet<&str> =
        blob.folders.iter().map(|f| f.id.as_str()).collect();
    blob.shelves.retain(|s| match &s.kind {
        ShelfKind::Virtual => true,
        ShelfKind::Folder { folder_id, .. } => folders.contains(folder_id.as_str()),
    });

    crate::shelf::sanitize(&mut blob.shelves);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::FolderOpts;
    use std::collections::{BTreeMap, HashSet};

    fn legacy(path: &str, page: u32, num: u32) -> RecentBook {
        RecentBook {
            path: path.to_string(),
            title: None,
            page,
            num_pages: num,
            fraction: None,
        }
    }

    fn shelf(id: &str, name: &str, kind: ShelfKind, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    #[test]
    fn an_empty_library_is_the_default_one() {
        let blob = LibraryBlob::default();
        assert!(blob.is_empty());
        assert!(blob.shelves.is_empty() && blob.folders.is_empty());
        assert_eq!(blob.view, LibraryView::default());
        assert!(!blob.awaiting_check(), "nothing to check is nothing pending");
    }

    #[test]
    fn a_migration_keeps_the_order_and_the_resume_point() {
        let v1 = vec![
            legacy("/books/dune.pdf", 42, 400),
            legacy("/books/notes.md", 1, 0),
        ];
        let blob = migrate_v1(v1, 1_000);
        assert_eq!(blob.books.len(), 2);
        assert_eq!(blob.books[0].path(), "/books/dune.pdf");
        assert_eq!(blob.books[0].page, 42);
        assert_eq!(blob.books[0].num_pages, 400);
        assert_eq!(blob.books[1].format, reader_core::format::Format::Markdown);
        // Read in place: nothing was copied anywhere.
        assert!(blob.books.iter().all(|b| !b.origin.is_stored()));
        assert!(blob.shelves.is_empty(), "All is the book list, not a shelf");
        assert_ne!(blob.books[0].id, blob.books[1].id);
    }

    #[test]
    fn a_migrated_book_says_so_until_it_has_been_measured() {
        let mut blob = migrate_v1(
            vec![legacy("/books/dune.pdf", 1, 1), legacy("/books/notes.md", 1, 1)],
            5,
        );
        assert!(blob.awaiting_check());
        assert!(blob.books.iter().all(|b| b.fp_pending));
        // The placeholder is derived from the address, so two migrated books
        // never collide on it — the ledger's fingerprint index is first-wins,
        // and a migration that collapsed every row onto one stamp would hide
        // all of them but one from every scan.
        assert_ne!(
            Fingerprint::placeholder("/a.pdf"),
            Fingerprint::placeholder("/b.pdf")
        );
        crate::book::sanitize(&mut blob.books);
        assert_eq!(blob.books.len(), 2, "both rows survive the dedupe");
        // A measured fingerprint clears the mark, one book at a time.
        blob.books[0].fp = Fingerprint::of(1024, 99, b"%PDF-1.7");
        blob.books[0].fp_pending = false;
        assert!(blob.awaiting_check(), "the second book is still a placeholder");
        blob.books[1].fp_pending = false;
        assert!(!blob.awaiting_check());
    }

    #[test]
    fn a_migration_drops_rows_with_no_address_and_clamps_the_page() {
        let blob = migrate_v1(
            vec![legacy("", 1, 1), legacy("   ", 1, 1), legacy("/a.pdf", 0, 9)],
            1,
        );
        assert_eq!(blob.books.len(), 1);
        assert_eq!(blob.books[0].page, 1);
    }

    #[test]
    fn an_impossible_fraction_does_not_survive_the_migration() {
        let mut row = legacy("/notes.md", 1, 0);
        row.fraction = Some(1.4);
        assert_eq!(migrate_v1(vec![row], 1).books[0].fraction, None);
    }

    #[test]
    fn a_v1_blob_parses_under_its_own_names() {
        let rows: Vec<RecentBook> = serde_json::from_str(
            r#"[{"path":"/a.pdf","title":"A","page":3,"numPages":9,"fraction":0.5}]"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].num_pages, 9);
        assert_eq!(rows[0].fraction, Some(0.5));
        // And with the fields the oldest builds did not write.
        let older: Vec<RecentBook> = serde_json::from_str(r#"[{"path":"/a.pdf"}]"#).unwrap();
        assert_eq!(older[0].page, 1);
        assert_eq!(older[0].title, None);
    }

    #[test]
    fn the_whole_library_round_trips_through_one_key() {
        let blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/books/dune.pdf", 4, 40)], 1).books,
            shelves: vec![shelf("s1", "Sci-fi", ShelfKind::Virtual, &[])],
            folders: vec![WatchedFolder {
                id: "f1".into(),
                root: "/books".into(),
                opts: FolderOpts::default(),
                placed: HashSet::new(),
                ignored: Vec::new(),
                shelf_map: BTreeMap::new(),
                last_seen: Vec::new(),
                scanned_ms: 0,
            }],
            view: LibraryView::default(),
        };
        let json = serde_json::to_string(&blob).unwrap();
        let back: LibraryBlob = serde_json::from_str(&json).unwrap();
        assert_eq!(back, blob);
    }

    #[test]
    fn a_blob_without_the_newer_halves_still_loads() {
        let blob: LibraryBlob = serde_json::from_str(r#"{"books":[]}"#).unwrap();
        assert_eq!(blob, LibraryBlob::default());
        let blob: LibraryBlob = serde_json::from_str("{}").unwrap();
        assert!(blob.is_empty());
    }

    #[test]
    fn a_shelf_member_that_names_no_book_is_dropped() {
        let mut blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/books/dune.pdf", 1, 1)], 1).books,
            shelves: Vec::new(),
            ..LibraryBlob::default()
        };
        // The member list has to name the ids the migration actually minted: a
        // hand-written "b0" is exactly the stale member this is testing for.
        let kept = blob.books[0].id.clone();
        blob.shelves = vec![shelf(
            "s1",
            "Sci-fi",
            ShelfKind::Virtual,
            &[kept.as_str(), "gone"],
        )];
        sanitize(&mut blob);
        assert_eq!(blob.shelves[0].books, vec![kept]);
    }

    #[test]
    fn a_folder_shelf_goes_with_its_folder_but_a_virtual_one_stays() {
        let mut blob = LibraryBlob {
            shelves: vec![
                shelf(
                    "s1",
                    "Books",
                    ShelfKind::Folder {
                        folder_id: "gone".into(),
                        rel: None,
                    },
                    &[],
                ),
                shelf("s2", "Mine", ShelfKind::Virtual, &[]),
            ],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        let ids: Vec<&str> = blob.shelves.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s2"]);
    }

    #[test]
    fn a_shelf_nested_in_a_stale_folder_shelf_survives_at_the_root() {
        let mut blob = LibraryBlob {
            shelves: vec![
                shelf(
                    "s1",
                    "Books",
                    ShelfKind::Folder {
                        folder_id: "gone".into(),
                        rel: None,
                    },
                    &[],
                ),
                Shelf {
                    parent: Some("s1".into()),
                    ..shelf("s2", "Mine", ShelfKind::Virtual, &[])
                },
            ],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        assert_eq!(blob.shelves.len(), 1);
        assert_eq!(blob.shelves[0].id, "s2");
        assert_eq!(
            blob.shelves[0].parent, None,
            "a parent that went with its folder is not a parent any more"
        );
    }

    #[test]
    fn two_rows_sharing_an_id_leave_one_book_and_one_member() {
        let mut blob = migrate_v1(vec![legacy("/a.pdf", 1, 1), legacy("/b.pdf", 1, 1)], 1);
        let shared = blob.books[0].id.clone();
        blob.books[1].id = shared.clone();
        blob.shelves = vec![shelf(
            "s1",
            "One",
            ShelfKind::Virtual,
            &[shared.as_str(), shared.as_str()],
        )];
        sanitize(&mut blob);
        assert_eq!(blob.books.len(), 1);
        assert_eq!(blob.shelves[0].books, vec![shared]);
    }

    #[test]
    fn sanitize_is_idempotent() {
        let mut blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/a.pdf", 1, 1)], 1).books,
            shelves: vec![shelf("s1", "One", ShelfKind::Virtual, &["nope"])],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        let once = blob.clone();
        sanitize(&mut blob);
        assert_eq!(blob, once);
    }
}
