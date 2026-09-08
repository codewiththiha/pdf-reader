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
use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use reader_core::format::Format;

use crate::state::library::{CoverImage, CoverMap, prune_covers};
use crate::state::AppState;
use library_core::book::Book;

/// Width the shelf renders a cover at. The same number
/// `crate::services::document::open::cover` uses: two widths would be two renders
/// of the same art and a cache that misses on the other one.
const COVER_WIDTH: f64 = 240.0;

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
}

/// Queue a cover render for every PDF book the shelf has no cover for.
///
/// Idempotent and cheap to call with the whole library: the two filters — already
/// covered, and not renderable — are applied before anything is queued, and a path
/// already waiting its turn is not queued twice. What this means in practice is
/// that an import, a restore, a relink and the startup pass all say the same one
/// sentence: "the shelf should look like its books".
/// The paths a backfill should queue: the books the engine can render a cover
/// for, minus the ones the shelf already has, in the order the library holds
/// them.
///
/// Split out because it is the whole of the policy — which books deserve a
/// render, and which are wasting one — and a policy this short is a policy a
/// test can hold in its hands.
fn wanted(books: &[Book], covers: &CoverMap) -> Vec<String> {
    books
        .iter()
        .filter(|b| b.format == Format::Pdf)
        .map(|b| b.path().to_string())
        .filter(|path| !covers.contains_key(path))
        .collect()
}

pub fn backfill_missing(state: AppState) {
    let wanted = state.library.books.with_untracked(|books| {
        state.library.covers.with_untracked(|covers| wanted(books, covers))
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

/// File one rendered cover under its path, keeping the cache inside its budget.
///
/// Shared by the queue below and by the open pipeline's own cover, because the
/// cap in `crate::state::library::COVER_CAP` is a localStorage quota and a quota
/// only holds if the insert that crosses it is the one that pays for it: pruning
/// on purge alone let a long shelf of opens grow the store past what fits.
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
    state.library.books.with_untracked(|books| {
        state
            .library
            .covers
            .update(|covers| prune_covers(books, covers));
    });
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
            if let Ok(cover) = engine::cover_data_url(&path, COVER_WIDTH).await {
                file_cover(state, path, cover.data_url, cover.width, cover.height);
            }
        }
        drain(state);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Fingerprint, Origin};

    fn book(path: &str, format: Format) -> Book {
        let len = path.len() as u64;
        Book {
            id: path.to_string(),
            fp: Fingerprint {
                size: len,
                mtime_ms: 0,
                head_hash: len as u32,
            },
            title: None,
            author: None,
            format,
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
    }

    #[test]
    fn a_cover_the_shelf_already_has_is_not_rendered_twice() {
        let books = vec![book("/a/dune.pdf", Format::Pdf)];
        let mut covers = CoverMap::default();
        covers.insert("/a/dune.pdf".to_string(), cover());
        assert!(wanted(&books, &covers).is_empty(), "an open already filed this one");
    }
}
