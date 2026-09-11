//! The library domain: the books, the shelves they are filed on, the folders
//! watched for new ones, the cover-art cache, and the view the shelf renders
//! in.
//!
//! The RULES are not here — they are `library_core`, which is pure and
//! host-tested. This module is the reactive half: the signals those rules are
//! applied to, the import dock's task list, and the cover cache's own budget.
//!
//! Kept OUT of `Settings` on purpose, for the reason the previous schema gave
//! and this one inherits: reading position changes on every page turn, while
//! `Settings` is the appearance blob that repaints and re-serialises on every
//! write. The library's own view knobs (columns, sort, cover fit) change at the
//! same pace as a page turn, so they ride with the library rather than joining
//! the settings blob — one localStorage key, one write schedule.
//!
//! That schedule is unchanged: reading progress saves on a debounce through
//! `crate::effects::reader::reading_progress`, and the last moments before a
//! teardown (a document closing, a book leaving the shelf) write immediately
//! through `crate::storage::persist_library`, because a debounced save there is
//! a save that may never land.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use library_core::blob::LibraryBlob;
use library_core::book::{Book, Row, book_rows};
use library_core::folder::WatchedFolder;
use library_core::id;
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::text::plural;
use library_core::view::LibraryView;
use library_core::wire::{ImportPhase, ImportProgress};

use crate::services::library::conflict::ConflictAsk;
use crate::time::now_ms;

/// How many covers the cache holds. A cover is a base64 JPEG of a few tens of
/// kilobytes, so this — not [`BOOKS_CAP`](library_core::book::BOOKS_CAP)
/// — is the library's real memory and quota budget. Past it the least recently
/// read covers go; they are derived, and reopening a book renders its page 1
/// again.
pub const COVER_CAP: usize = 60;

/// Persisted cover art for one book: the first page rendered to a small JPEG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// The cover-art cache: page-1 JPEG data URLs keyed by the address a book is
/// read from. Behind an `Arc` because a cover is tens of kilobytes and the map
/// is read out of a signal on every shelf render and cloned whole before every
/// save; sharing the images makes those reads pointer copies.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

/// Bring the cover cache back inside its budget, and drop the covers of books
/// that are no longer in the library.
///
/// Two jobs in one pass because they are the same question — "is this cover
/// still wanted?" — and the cap is only a real budget if an evicted book takes
/// its art with it. The survivors are the most recently read, so a shelf the
/// reader is scrolling through keeps the covers they are looking at.
pub fn prune_covers(rows: &[Row], covers: &mut CoverMap) {
    // The books, and only the books: a link has no address and no page 1, so
    // it holds no art and keeps none alive.
    let books: Vec<&Book> = book_rows(rows).collect();
    let live: HashSet<&str> = books.iter().map(|b| b.path()).collect();
    covers.retain(|path, _| live.contains(path.as_str()));
    if covers.len() <= COVER_CAP {
        return;
    }
    let mut by_recency: Vec<(String, u64)> = books
        .iter()
        .map(|b| (b.path().to_string(), b.last_read_ms.max(b.added_ms)))
        .collect();
    by_recency.sort_by_key(|(_, stamp)| std::cmp::Reverse(*stamp));
    let keep: HashSet<&str> = by_recency
        .iter()
        .take(COVER_CAP)
        .map(|(path, _)| path.as_str())
        .collect();
    covers.retain(|path, _| keep.contains(path.as_str()));
}

/// Which half of an import a dock card is showing. The shell reports
/// [`ImportPhase`] for the two it can see; `Done` and `Failed` are the
/// frontend's, because only the side that owns the task list knows when the
/// whole run — scan, then copies, then the state write — has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    Scanning,
    Copying,
    Done,
    Failed,
}

impl TaskPhase {
    /// From a shell beat. The shell never says "done", so a beat always means
    /// still working.
    fn of(phase: ImportPhase) -> Self {
        match phase {
            ImportPhase::Scan => TaskPhase::Scanning,
            ImportPhase::Copy => TaskPhase::Copying,
        }
    }

    /// True once the card has nothing left to report and can leave the dock.
    pub fn is_finished(self) -> bool {
        matches!(self, TaskPhase::Done | TaskPhase::Failed)
    }
}

/// One card in the import dock.
///
/// Deliberately a plain value and not a projection of the shell's beat: the run
/// outlives the beat (it ends with a state write the shell knows nothing
/// about), and a card that only mirrored the last event would sit at "100%"
/// while the books were still being filed.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTask {
    /// The id the shell echoes on every beat, so two runs in flight never mix.
    pub id: String,
    /// What the card calls the run: the folder's name, or "3 files".
    pub label: String,
    pub phase: TaskPhase,
    pub done: u32,
    pub total: u32,
    /// How many of the run's files went to the conflict sheet instead of
    /// landing — questions the reader still owes an answer to. The finished
    /// card says so rather than claiming an import that is waiting on
    /// somebody; see `crate::services::library::conflict`.
    pub waiting: u32,
    /// The file being worked on — the card's second line.
    pub name: String,
    pub error: Option<String>,
}

impl ImportTask {
    /// A run that has just been handed to the shell: scanning, total unknown.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            phase: TaskPhase::Scanning,
            done: 0,
            total: 0,
            waiting: 0,
            name: String::new(),
            error: None,
        }
    }

    /// Fold one shell beat into the card. A beat for another run is the
    /// caller's problem (it looks the card up by id first).
    pub fn beat(&mut self, beat: &ImportProgress) {
        self.phase = TaskPhase::of(beat.phase);
        self.done = beat.done;
        self.total = beat.total;
        self.name = beat.name.clone();
    }

    /// The run finished: everything the shell reported is behind it.
    pub fn finish(&mut self) {
        self.phase = TaskPhase::Done;
        self.error = None;
    }

    /// The run could not finish.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = TaskPhase::Failed;
        self.error = Some(message.into());
    }

    /// Fraction complete, or `None` while the total is unknown — which is the
    /// whole of a scan, and is what makes the card's ring indeterminate.
    pub fn fraction(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some((f64::from(self.done) / f64::from(self.total)).clamp(0.0, 1.0))
    }

    /// The number the ring prints, when there is one to print.
    pub fn percent(&self) -> Option<u32> {
        self.fraction().map(|f| (f * 100.0).round() as u32)
    }

    /// The card's first line: what is happening, and to how much.
    pub fn headline(&self) -> String {
        match self.phase {
            TaskPhase::Scanning => "Scanning…".to_string(),
            TaskPhase::Failed => "Import failed".to_string(),
            TaskPhase::Done => {
                // A run that raised questions is not an import that finished:
                // the books it asked about are waiting on the reader, and a
                // card that said "Imported" over them would be a promise the
                // shelf has to take back.
                if self.waiting > 0 {
                    format!(
                        "{} waiting for your choice",
                        plural(self.waiting as usize, "book", "books")
                    )
                } else {
                    format!("Imported {}", plural(self.total as usize, "book", "books"))
                }
            }
            TaskPhase::Copying => match self.total {
                0 => "Importing…".to_string(),
                n => format!("Importing {} of {n}", self.done.min(n)),
            },
        }
    }
}

/// The library domain's signals.
///
/// Four of these persist together as one [`LibraryBlob`] and are separate
/// signals anyway: the shelf re-renders when a book's resume point moves, and
/// nothing else should. The rest are the page's own and a restart forgets them — a
/// search that survived would be one the reader did not type, a dock card that
/// survived would report an import that finished last week, and a selection that
/// survived would be a set of books the reader cannot see selected.
#[derive(Clone, Copy)]
pub struct LibraryState {
    /// Every ROW, in the order the "All" shelf shows them: the books, and the
    /// links that point at them. This list IS the All order; there is no shelf
    /// row for it.
    pub books: RwSignal<Vec<Row>>,
    pub shelves: RwSignal<Vec<Shelf>>,
    pub folders: RwSignal<Vec<WatchedFolder>>,
    /// The view knobs, persisted with the books.
    pub view: RwSignal<LibraryView>,
    /// Cover art (page-1 JPEG data URLs) keyed by address.
    pub covers: RwSignal<CoverMap>,
    /// The titlebar search.
    pub query: RwSignal<String>,
    /// Which shelf the page is drilled into; [`ALL_SHELF`] at the root.
    pub shelf: RwSignal<String>,
    /// The import dock's cards, oldest first.
    pub tasks: RwSignal<Vec<ImportTask>>,
    /// The book to scroll to and light up, with a nonce so revealing the same
    /// book twice in a row re-triggers. Written by a "show it in its shelf"
    /// action, cleared by the shelf that scrolled to it.
    pub reveal: RwSignal<Option<(String, u64)>>,
    /// Whether a long-press has put the shelf into multi-select. While it is on a
    /// click toggles instead of opening, and the action bar owns the bottom-right
    /// corner.
    pub selecting: RwSignal<bool>,
    /// The selected book ids. A set rather than a list because toggling is the
    /// high-frequency operation and "is this one selected" is asked by every card
    /// on every repaint.
    pub selected: RwSignal<HashSet<String>>,
    /// The name collision the sheet is asking about, waiting for the reader's
    /// answer. Raised by the services — a drop, a filing, an import — rather
    /// than by a component, which is why it lives here and not in a sheet's own
    /// handle: an import asks from inside a spawned future that outlived every
    /// component. See `crate::services::library::conflict`.
    pub conflict: RwSignal<Option<ConflictAsk>>,
    /// The collisions behind the one on screen.
    ///
    /// One question at a time is the sheet's whole shape, and a batch — a drag
    /// of four books, an import of ten files — can raise several. Without
    /// somewhere to put the rest, the second raise would overwrite the first
    /// and a placement would vanish, which is the one thing the sheet exists
    /// to stop. Answering pops the next one onto the screen; Cancel drops them,
    /// which is what Cancel has always meant — the placements already answered
    /// keep their answers and the ones not asked simply do not land.
    pub conflict_waiting: RwSignal<Vec<ConflictAsk>>,
    /// The pair of [`Self::conflict`] that the overlay lane and the Escape rule
    /// can hold: the lane registry speaks in booleans. Every writer of the two
    /// goes through the conflict service's `raise` and `cancel`, so while the
    /// sheet is up they cannot drift; a lane arbitration that closes the sheet
    /// behind its back leaves a payload nobody reads, and the next raise
    /// replaces it.
    pub conflict_open: RwSignal<bool>,
}

impl Default for LibraryState {
    /// Hand-written for one field: the page opens drilled out of nothing, which
    /// is the root shelf, and [`ALL_SHELF`] is its id. A derived `String::new()`
    /// would be a shelf that does not exist, and the breadcrumb would render an
    /// empty crumb.
    fn default() -> Self {
        Self {
            books: RwSignal::new(Vec::new()),
            shelves: RwSignal::new(Vec::new()),
            folders: RwSignal::new(Vec::new()),
            view: RwSignal::new(LibraryView::default()),
            covers: RwSignal::new(CoverMap::default()),
            query: RwSignal::new(String::new()),
            shelf: RwSignal::new(ALL_SHELF.to_string()),
            tasks: RwSignal::new(Vec::new()),
            reveal: RwSignal::new(None),
            selecting: RwSignal::new(false),
            selected: RwSignal::new(HashSet::new()),
            conflict: RwSignal::new(None),
            conflict_waiting: RwSignal::new(Vec::new()),
            conflict_open: RwSignal::new(false),
        }
    }
}

impl LibraryState {
    /// The library as one persisted value: an untracked read of every signal
    /// that belongs in the blob. Storage takes a value and writes it, so this
    /// is the only place the reactive graph and localStorage meet.
    pub fn snapshot(&self) -> LibraryBlob {
        LibraryBlob {
            books: self.books.get_untracked(),
            shelves: self.shelves.get_untracked(),
            folders: self.folders.get_untracked(),
            view: self.view.get_untracked(),
        }
    }

    /// The two lists a collision is asked of, read once and untracked: the
    /// rows and the shelves. One read of each rather than three nested reads
    /// is one chance to see a library, and a rule asked of a shelf list and a
    /// row list that were read at different moments can be asked of two.
    pub fn snapshot_rows(&self) -> (Vec<Row>, Vec<Shelf>) {
        (self.books.get_untracked(), self.shelves.get_untracked())
    }

    /// The row an id names, whichever kind it is — what an open, a drag and a
    /// removal all start from.
    pub fn row(&self, row_id: &str) -> Option<Row> {
        self.books
            .with_untracked(|rows| library_core::book::find_row(rows, row_id).cloned())
    }

    /// The name a row shows, and the name a collision compares. Empty for a
    /// row that is not there, which is what makes a drag of a row another
    /// surface just removed a no-op rather than a placement of nothing.
    pub fn row_name(&self, row_id: &str) -> String {
        self.row(row_id).map_or_else(String::new, |r| r.display_name())
    }

    /// Give a row a new name: a book's title, a link's own. What the sheet's
    /// *add as new* answer does to a row it is moving, and the only rename the
    /// library performs on a reader's behalf.
    pub fn rename_row(&self, row_id: &str, name: &str) {
        self.books.update(|rows| {
            let Some(row) = library_core::book::find_row_mut(rows, row_id) else {
                return;
            };
            match row {
                Row::Book(b) => b.title = Some(name.to_string()),
                Row::Link { name: own, .. } => *own = name.to_string(),
            }
        });
    }

    /// Put a link to `target` on `shelf_id`, and return its id.
    ///
    /// The name is the target's own at this moment, which is what makes the
    /// row recognisable on the shelf beside the book it points at; a rename of
    /// the book later does not rewrite it, because a link is a row the reader
    /// placed and not a view of another row.
    pub fn add_link(&self, name: &str, target: &str, shelf_id: &str) -> String {
        let now = now_ms();
        let link_id = id::next_id(now);
        let made = link_id.clone();
        self.books.update(|rows| {
            rows.push(Row::link(link_id, name.to_string(), target.to_string(), now));
        });
        if shelf_id != ALL_SHELF {
            self.shelves.update(|shelves| {
                if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
                    shelf::shelf_add(shelf, &made);
                }
            });
        }
        crate::storage::persist_library(*self);
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Fingerprint, Origin};
    use reader_core::format::Format;

    /// A book row: the cover cache is keyed by address, and a link has none.
    fn book(path: &str, last_read: u64) -> Row {
        Row::Book(book_value(path, last_read))
    }

    fn book_value(path: &str, last_read: u64) -> Book {
        let len = path.len() as u64;
        Book {
            id: path.to_string(),
            fp: Fingerprint {
                size: len,
                mtime_ms: last_read,
                head_hash: len as u32,
            },
            title: None,
            author: None,
            format: Format::Pdf,
            origin: Origin::Linked {
                src: path.to_string(),
            },
            added_ms: last_read,
            last_read_ms: last_read,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
        }
    }

    fn cover() -> Arc<CoverImage> {
        Arc::new(CoverImage {
            data_url: "data:image/jpeg;base64,x".to_string(),
            width: 240.0,
            height: 320.0,
        })
    }

    #[test]
    fn a_cover_outlives_nothing_it_does_not_belong_to() {
        let books = vec![book("/a.pdf", 1), book("/b.pdf", 2), Row::link(
            "l1".into(),
            "A".into(),
            "/a.pdf".into(),
            3,
        )];
        let mut covers: CoverMap = [
            ("/a.pdf".to_string(), cover()),
            ("/b.pdf".to_string(), cover()),
            ("/gone.pdf".to_string(), cover()),
        ]
        .into_iter()
        .collect();
        prune_covers(&books, &mut covers);
        assert_eq!(covers.len(), 2, "a link keeps no art alive and holds none");
        assert!(!covers.contains_key("/gone.pdf"));
    }

    #[test]
    fn the_cap_keeps_the_most_recently_read() {
        let books: Vec<Row> = (0..(COVER_CAP + 5))
            .map(|i| book(&format!("/books/{i}.pdf"), i as u64))
            .collect();
        let mut covers: CoverMap = books
            .iter()
            .filter_map(Row::book)
            .map(|b| (b.path().to_string(), cover()))
            .collect();
        prune_covers(&books, &mut covers);
        assert_eq!(covers.len(), COVER_CAP);
        // The five never-read-again books at the head of the list are the ones
        // that went.
        assert!(!covers.contains_key("/books/0.pdf"));
        assert!(!covers.contains_key("/books/4.pdf"));
        assert!(covers.contains_key("/books/5.pdf"));
    }

    #[test]
    fn a_beat_moves_the_card_and_never_finishes_it() {
        let mut task = ImportTask::new("t1", "Books");
        assert_eq!(task.phase, TaskPhase::Scanning);
        assert_eq!(task.fraction(), None, "a scan has no total yet");
        assert_eq!(task.headline(), "Scanning…");

        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 12,
            total: 48,
            name: "dune.pdf".into(),
        });
        assert_eq!(task.phase, TaskPhase::Copying);
        assert_eq!(task.percent(), Some(25));
        assert_eq!(task.headline(), "Importing 12 of 48");
        assert_eq!(task.name, "dune.pdf");
        assert!(!task.phase.is_finished());

        task.finish();
        assert_eq!(task.phase, TaskPhase::Done);
        assert_eq!(task.headline(), "Imported 48 books");
        assert!(task.phase.is_finished());
    }

    #[test]
    fn a_failed_run_says_so_and_stops_counting() {
        let mut task = ImportTask::new("t1", "Books");
        task.fail("no such folder");
        assert_eq!(task.phase, TaskPhase::Failed);
        assert_eq!(task.error.as_deref(), Some("no such folder"));
        assert_eq!(task.headline(), "Import failed");
        assert!(task.phase.is_finished());
    }

    #[test]
    fn one_book_reads_as_one_book() {
        let mut task = ImportTask::new("t1", "Books");
        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 1,
            total: 1,
            name: "a.pdf".into(),
        });
        task.finish();
        assert_eq!(task.headline(), "Imported 1 book");
    }

    #[test]
    fn a_run_that_asked_ends_on_the_question() {
        // Ten files measured, eight landed, two went to the conflict sheet:
        // the card counts what landed and ends on what is still owed.
        let mut task = ImportTask::new("t1", "10 files");
        task.total = 8;
        task.done = 8;
        task.waiting = 2;
        task.finish();
        assert_eq!(task.headline(), "2 books waiting for your choice");
        // One question reads as one book...
        task.waiting = 1;
        assert_eq!(task.headline(), "1 book waiting for your choice");
        // ...and an import that asked nothing claims its books as before.
        task.waiting = 0;
        assert_eq!(task.headline(), "Imported 8 books");
    }

    #[test]
    fn a_count_past_the_total_clamps_rather_than_boasts() {
        let mut task = ImportTask::new("t1", "Books");
        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 40,
            total: 10,
            name: String::new(),
        });
        assert_eq!(task.percent(), Some(100));
        assert_eq!(task.headline(), "Importing 10 of 10");
    }
}
