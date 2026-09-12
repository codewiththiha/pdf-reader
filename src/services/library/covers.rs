//! The shelf's covers, rendered away from the reader.
//!
//! A cover is page 1 of a book as a small JPEG. The engine can render one from a
//! path with nothing open — `pdf_engine::api::cover_data_url` opens its own
//! document task, paints page 1 and destroys the task again — which is what makes
//! a cover at IMPORT time possible at all. Before this module a cover appeared the
//! first time a book was opened and not a moment before, so a freshly imported
//! shelf was a shelf of stylised fallbacks until the reader had visited every book
//! on it, and a folder card's plate of four covers stayed empty for as long as its
//! books stayed unopened.
//!
//! A render that fails gets one retry at the back of the queue, and the shelf
//! re-asks for whatever it is missing every time the library page is looked at and
//! every time the window regains focus — so a cover that failed for a second's
//! reason converges on the next visit rather than staying an empty cell forever.
//!
//! The renders run ONE at a time, off a queue, for two reasons. A cover is a
//! `getDocument` in a worker plus a canvas paint on the main thread, and an import
//! of three hundred books firing three hundred of them at once is an import the
//! reader cannot scroll past. And the cover store is one growing base64 blob that
//! every save re-serialises, so it is written once when the queue drains rather
//! than once per cover.
//!
//! Books that are not PDFs are skipped rather than attempted: the engine is
//! pdf.js, a Markdown file fails its parse, and a failure files no cover — so an
//! unfiltered queue would re-attempt every text book on every import and never
//! remember having failed. Those books keep the stylised fallback cover, which is
//! their cover.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use reader_core::format::Format;

use crate::state::library::{CoverImage, CoverMap};
use crate::state::AppState;
use library_core::book::{Book, Row, book_rows};

/// Width the shelf renders a cover at. One number for both renders of the same
/// art — the import queue's and the open pipeline's
/// (`crate::services::document::open::cover`) — because two widths would be two
/// renders of one cover and a cache that misses on the other one.
pub(crate) const COVER_WIDTH: f64 = 240.0;

/// How many covers the cache keeps past a prune. The budget is a localStorage
/// quota rather than a memory one — the covers persist under their own key
/// (`crate::storage::persist_covers`) — and the survivors are the most
/// recently read, so a shelf the reader is scrolling through keeps the covers
/// they are looking at.
pub const COVER_CAP: usize = 60;

/// Bring the cover cache back inside its budget, and drop the covers of books
/// that are no longer in the library.
///
/// Two jobs in one pass because they are the same question — "is this cover
/// still wanted?" — and the cap is only a real budget if an evicted book takes
/// its art with it. The policy lives beside the queue that fills the cache:
/// one module owns what the cache holds, from the render it queues to the
/// eviction it owes.
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

thread_local! {
    /// Paths waiting for a render, in the order they were asked for.
    static QUEUE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Whether a render is in flight. The queue is drained by a chain of tasks —
    /// each finished render starts the next — so exactly one is ever alive, and
    /// this is what keeps an import of any size from becoming a stampede.
    static DRAINING: RefCell<bool> = const { RefCell::new(false) };
    /// Whether anything has been filed since the last save. The drain writes the
    /// store once, when it empties, and not at all when every render in the batch
    /// failed.
    static DIRTY: RefCell<bool> = const { RefCell::new(false) };
    /// Paths whose render has already failed once and been re-queued. One retry
    /// each: a cover can fail for a reason that is true for a second — a file
    /// still being copied, a worker still warming up — and a preview that gave up
    /// on the first shrug stays empty forever, but a queue that re-attempts a
    /// genuinely unrenderable file forever is a queue that never drains.
    static RETRIES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// The paths a backfill should queue: the books the engine can render a cover
/// for, minus the ones the shelf already has, in the order the library holds
/// them.
///
/// Split out because it is the whole of the policy — which books deserve a
/// render, and which are wasting one — and a policy this short is a policy a
/// test can hold in its hands. A link is not in the answer and cannot be: it
/// has no address to render from and no page 1, and the art of the book it
/// points at is already queued by that book's own row.
fn wanted(rows: &[Row], covers: &CoverMap) -> Vec<String> {
    book_rows(rows)
        .filter(|b| b.format == Format::Pdf)
        .map(|b| b.path().to_string())
        .filter(|path| !covers.contains_key(path))
        .collect()
}

/// Queue a cover render for every PDF book the shelf has no cover for.
///
/// Idempotent and cheap to call with the whole library: the two filters — already
/// covered, and not renderable — are applied before anything is queued, and a path
/// already waiting its turn is not queued twice. What this means in practice is
/// that an import, a restore, a relink and the startup pass all say the same one
/// sentence: "the shelf should look like its books".
pub fn backfill_missing(state: AppState) {
    // A new ask is a new retry budget: whatever failed last time is worth one
    // more attempt now, because the most likely reason it failed was timing.
    RETRIES.with(|retries| retries.borrow_mut().clear());
    let wanted = state.library.books.with_untracked(|rows| {
        state.library.covers.with_untracked(|covers| wanted(rows, covers))
    });
    if wanted.is_empty() {
        return;
    }
    QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        for path in wanted {
            if !queue.contains(&path) {
                queue.push(path);
            }
        }
    });
    // Start the chain only when nothing is running: a live chain always comes
    // back to `drain` after its current render, and picks up whatever arrived in
    // the meantime. Starting a second chain here would be a second concurrent
    // render, which is the one thing the queue exists to prevent.
    let start = DRAINING.with(|draining| {
        let mut draining = draining.borrow_mut();
        if *draining {
            false
        } else {
            *draining = true;
            true
        }
    });
    if start {
        drain(state);
    }
}

/// Bring the cover cache back inside its budget: one door, so every caller —
/// a purge, a relink, a book joining, a cover landing — prunes the same way.
///
/// The cap in [`COVER_CAP`] is a localStorage quota, and a
/// quota only holds if the insert that crosses it is the one that pays for it:
/// pruning on purge alone let a long shelf of opens grow the store past what
/// fits. Does NOT persist — the caller decides when the store is written, and
/// the queue below batches that to once per drain.
pub(crate) fn prune_now(state: AppState) {
    state.library.books.with_untracked(|rows| {
        state
            .library
            .covers
            .update(|covers| prune_covers(rows, covers));
    });
}

/// File one rendered cover under its path, keeping the cache inside its budget.
///
/// Shared by the queue below and by the open pipeline's own cover: the two
/// renders of one art go through one door, or the cache grows two ways.
pub fn file_cover(state: AppState, path: String, data_url: String, width: f64, height: f64) {
    state.library.covers.update(|covers| {
        covers.insert(
            path,
            Arc::new(CoverImage {
                data_url,
                width,
                height,
            }),
        );
    });
    prune_now(state);
    DIRTY.with(|dirty| *dirty.borrow_mut() = true);
}

/// Render the next queued cover, then hand over to the one after it.
fn drain(state: AppState) {
    let next = QUEUE.with(|queue| queue.borrow_mut().pop());
    let Some(path) = next else {
        DRAINING.with(|draining| *draining.borrow_mut() = false);
        if DIRTY.with(|dirty| {
            let was = *dirty.borrow();
            *dirty.borrow_mut() = false;
            was
        }) {
            if let Err(e) = state
                .library
                .covers
                .with_untracked(crate::storage::save_covers)
            {
                e.report();
            }
        }
        return;
    };
    spawn_local(async move {
        // An open may have filed a cover while this path waited its turn, and
        // re-rendering it would be a worker and a canvas spent on art the shelf
        // already has.
        let have = state
            .library
            .covers
            .with_untracked(|covers| covers.contains_key(&path));
        if !have {
            match engine::cover_data_url(&path, COVER_WIDTH).await {
                Ok(cover) => {
                    RETRIES.with(|retries| retries.borrow_mut().remove(&path));
                    file_cover(state, path, cover.data_url, cover.width, cover.height);
                }
                Err(_) => {
                    let first_failure = RETRIES.with(|retries| retries.borrow_mut().insert(path.clone()));
                    if first_failure {
                        QUEUE.with(|queue| queue.borrow_mut().push(path.clone()));
                    }
                }
            }
        }
        drain(state);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Book, Fingerprint, Origin};

    /// A book row: the cover queue reads a shelf's rows and skips the links.
    fn book(path: &str, format: Format) -> Row {
        Row::Book(book_value(path, format))
    }

    fn book_value(path: &str, format: Format) -> Book {
        let len = path.len() as u64;
        Book {
            fp: Fingerprint {
                size: len,
                mtime_ms: 0,
                head_hash: len as u32,
            },
            format,
            origin: Origin::Linked {
                src: path.to_string(),
            },
            ..library_core::testkit::book(path)
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
    fn only_pdfs_without_a_cover_are_worth_a_render() {
        let books = vec![
            book("/a/dune.pdf", Format::Pdf),
            book("/b/notes.md", Format::Markdown),
            book("/c/log.txt", Format::Text),
            book("/d/second.pdf", Format::Pdf),
        ];
        // A Markdown or a text file is not a render the engine can finish, and a
        // failure files nothing, so queueing one would re-attempt it forever.
        let asked = wanted(&books, &CoverMap::default());
        assert_eq!(asked, vec!["/a/dune.pdf".to_string(), "/d/second.pdf".to_string()]);
        // A link has no page 1 of its own to render, and the art of the book it
        // points at is that book's row's business.
        let mut with_link = books;
        with_link.push(Row::link("l1".into(), "Dune".into(), "/a/dune.pdf".into(), 5));
        assert_eq!(
            wanted(&with_link, &CoverMap::default()),
            vec!["/a/dune.pdf".to_string(), "/d/second.pdf".to_string()]
        );
    }

    #[test]
    fn a_cover_the_shelf_already_has_is_not_rendered_twice() {
        let books = vec![book("/a/dune.pdf", Format::Pdf)];
        let mut covers = CoverMap::default();
        covers.insert("/a/dune.pdf".to_string(), cover());
        assert!(wanted(&books, &covers).is_empty(), "an open already filed this one");
    }

    /// A book row with a reading history: the cap evicts by recency, so the
    /// stamps are the fact under test.
    fn book_read(path: &str, last_read: u64) -> Row {
        let len = path.len() as u64;
        Row::Book(Book {
            fp: Fingerprint {
                size: len,
                mtime_ms: last_read,
                head_hash: len as u32,
            },
            origin: Origin::Linked {
                src: path.to_string(),
            },
            added_ms: last_read,
            last_read_ms: last_read,
            ..library_core::testkit::book(path)
        })
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
        let books = vec![
            book_read("/a.pdf", 1),
            book_read("/b.pdf", 2),
            Row::link("l1".into(), "A".into(), "/a.pdf".into(), 3),
        ];
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
            .map(|i| book_read(&format!("/books/{i}.pdf"), i as u64))
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
}
