//! Importing: running the shell's measurements through the library's ledger and
//! writing the answer to the state.
//!
//! One path for all three ways books arrive — the folder sheet, a handful of
//! files from the picker or a drop, and a rescan of a watched folder when the
//! window regains focus. They differ in where the measurements come from and
//! nothing else, so the deciding and the writing happen once, here.
//!
//! Two rules this module exists to keep:
//!
//!   * **nothing is written until the whole answer is known.** The scan, the
//!     ledger and the copies all run against local copies of the three lists,
//!     and the state is set once at the end. A shelf that filled in as the
//!     import went would repaint per file, and a failure half way through would
//!     leave the library holding books whose bytes never arrived.
//!   * **a rescan is invisible unless it found something.** A quiet run
//!     ([`rescan_watched`]) never raises a dock card, a toast or a state write
//!     for a folder nothing changed in — which, on every window focus, is nearly
//!     all of them.
//!   * **the tree on disk is the tree on the shelf — until a hand moves one.**
//!     A folder import mints the whole chain of shelves between the watched root
//!     and each file's subfolder, and a rescan re-hangs the folder's shelves on
//!     the rung their `rel` names, passing by the ones the reader moved by hand
//!     (`Shelf::manual_parent`), so importing "1" that holds "2", "3" and four
//!     books yields "1" at the root with "2", "3" and the books inside it — one
//!     logic, one tree, rather than a flat shelf list grown beside a nested one.
//!     Virtual shelves are the reader's own and no scan ever rearranges them.
//!   * **an ask outranks a removal.** The tombstones a removal writes are an
//!     answer to the passive rescan — "stay quiet about this file" — and not to
//!     the reader picking the same folder again a week later. [`Asked::Explicitly`]
//!     runs the import's own ledger table ([`ledger::diff_import`]), where those
//!     tombstones stand aside, and the tombstone is lifted when each book
//!     actually lands rather than when it is merely asked for. Without this,
//!     emptying a watched folder's shelf and importing the folder again returns
//!     nothing at all, silently: a broken import wearing a rule's clothes.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{
    Book, Fingerprint, Origin, Row, add_book, apply_check, book_rows, book_rows_mut,
};
use library_core::conflict::Arrival;
use library_core::folder::{FolderOpts, WatchedFolder, parent_key};
use library_core::id;
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};
use library_core::wire::{PathCheck, StoreRequest};
use reader_core::format::{Format, is_supported_path};

use super::conflict;
use super::{file_name, folder_label};
use crate::services::library as wire;
use crate::state::library::ImportTask;
use crate::state::AppState;

/// Who asked for a folder run, which is what the tombstones mean.
///
/// One type rather than a boolean at the call site because the two runs are not
/// two settings of one thing: they answer different questions, and a reader of
/// `run_folder(state, task, root, opts, true)` cannot tell which question `true`
/// was answering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// The reader picked the folder, or dropped it on the window. An explicit
    /// ask overrides the tombstones their own earlier removals wrote.
    Explicitly,
    /// The window regaining focus asked. This is exactly the case a tombstone
    /// exists for — a removed book must stay removed on its own — so they hold.
    OnFocus,
}

/// A run's id. The shell echoes it on every progress beat, so two imports in
/// flight never mix their counts, and the dock can look a card up by it.
fn task_id() -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    format!(
        "t{:x}-{}",
        js_sys::Date::now() as u64,
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

thread_local! {
    /// The roots a folder run is currently walking. One run per root, claimed
    /// synchronously and released when the run's future drops — see
    /// [`claim_root`].
    static RUNNING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// A claimed root, released when the run's future ends however it ends — a
/// completion, a failure or a panic all drop the guard the same way.
struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
    }
}

/// Claim `root` for one run, or answer `None` when one is already in flight.
///
/// Two concurrent walks of one folder are two snapshots of the same ledger row
/// and two writes back to it, and the second write drops whatever the first
/// run placed — a `placed` set that lost an entry re-adds a book the reader
/// already filed, and a tombstone that lost one resurrects a book they
/// removed. The check-and-claim is one synchronous step (the webview is
/// single-threaded), so two runs started in the same tick cannot both pass it.
fn claim_root(root: &str) -> Option<RootClaim> {
    RUNNING
        .with(|running| running.borrow_mut().insert(root.to_string()))
        .then(|| RootClaim(root.to_string()))
}

/// What a shelf cut from a subfolder is called: the subfolder's own name, or the
/// watched folder's name for the shelf at its root.
fn shelf_name(key: &str, root: &str) -> String {
    match key.rsplit('/').next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => folder_label(root),
    }
}

/// The `rel` a folder shelf records: `None` at the watched root, so a rescan can
/// tell that shelf from a subfolder that happens to be named like the root.
fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

// ---------------------------------------------------------------------------
// The dock's cards. Written from here rather than from the dock: the dock is a
// view, and a view that owned the lifecycle of the thing it renders would have
// to outlive the import it is reporting on.
// ---------------------------------------------------------------------------

fn push_task(state: AppState, task: ImportTask) {
    state.library.tasks.update(|tasks| tasks.push(task));
}

fn update_task(state: AppState, id: &str, change: impl FnOnce(&mut ImportTask) + 'static) {
    let id = id.to_string();
    state.library.tasks.update(|tasks| {
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            change(task);
        }
    });
}

/// Take a card out of the dock. The dock asks for this on a timer of its own;
/// nothing here decides how long a reader gets to look at a finished import.
pub fn dismiss_task(state: AppState, id: &str) {
    let id = id.to_string();
    state
        .library
        .tasks
        .update(|tasks| tasks.retain(|t| t.id != id));
}

fn fail(state: AppState, task: &str, message: String, quiet: bool) {
    if quiet {
        // A watched folder that cannot be read is not news the reader asked
        // for, and it fails again on the next focus. Say it once, on the
        // console, where a bug report can find it.
        web_sys::console::warn_1(&format!("[library] rescan failed: {message}").into());
        return;
    }
    let toast = message.clone();
    update_task(state, task, move |t| t.fail(message));
    state.toast(toast);
}

// ---------------------------------------------------------------------------
// The three ways in.
// ---------------------------------------------------------------------------

/// Import a folder, with the options the sheet was filled in with. Returns
/// immediately: the dock owns the feedback from here on.
pub fn import_folder(state: AppState, root: String, opts: FolderOpts) {
    // A folder already being imported is an import already answering this ask:
    // its card is on the dock and its walk is the same tree. Racing it would
    // clobber its ledger write, so the second ask says so instead.
    let Some(claim) = claim_root(&root) else {
        state.toast(format!("{} is already being imported.", folder_label(&root)));
        return;
    };
    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), folder_label(&root)));
    spawn_local(async move {
        // Held for the whole run: the drop is the release, on every exit path.
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::Explicitly).await;
    });
}

/// Import files picked from the dialog or dropped on the library.
///
/// Read in place, always: there is no folder to rescan and no structure to
/// preserve, so a copy would cost disk and buy nothing. `target` is the shelf a
/// drop landed on; `None` files onto no shelf, which leaves the books in "All"
/// and nowhere else — the honest answer for a handful of loose files.
pub fn import_files(state: AppState, paths: Vec<String>, target: Option<String>) {
    if paths.is_empty() {
        return;
    }
    let task = task_id();
    let label = match paths.len() {
        1 => file_name(&paths[0]),
        n => format!("{n} files"),
    };
    push_task(state, ImportTask::new(task.clone(), label));
    spawn_local(async move {
        run_files(state, task, paths, target).await;
    });
}

/// Re-scan every watched folder. Called on startup (from [`verify_library`],
/// never before it) and whenever the window regains focus.
pub fn rescan_watched(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // A library still carrying placeholder fingerprints cannot be diffed: a real
    // fingerprint matches no placeholder, so every book a watched folder already
    // holds would be added a second time. `verify_library` measures first and
    // calls this itself once it has.
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
        return;
    }
    let watched: Vec<(String, FolderOpts)> = state
        .library
        .folders
        .get_untracked()
        .iter()
        .filter(|f| f.opts.watch)
        .map(|f| (f.root.clone(), f.opts.clone()))
        .collect();
    for (root, opts) in watched {
        // A folder a previous run is still walking keeps its walk: a rescan is
        // a question, and the run in flight is already answering it.
        let Some(claim) = claim_root(&root) else {
            continue;
        };
        let task = task_id();
        spawn_local(async move {
            let _claim = claim;
            run_folder(state, task, root, opts, Asked::OnFocus).await;
        });
    }
}

/// Measure every address the library holds, once, at startup.
///
/// This is what makes `missing` true, what replaces a migrated book's
/// placeholder fingerprint with a real one, and therefore what has to happen
/// before any rescan. It ends by calling [`rescan_watched`] itself, so that
/// order is a property of this function rather than of whoever remembers to call
/// the two in the right sequence.
pub fn verify_library(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // The addresses to ask about: a link has none, and a pointer at a book is
    // as alive or as dead as the book it points at, which the book's own row
    // is already in this list to answer for.
    let paths: Vec<String> = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).map(|b| b.path().to_string()).collect());
    if paths.is_empty() {
        rescan_watched(state);
        return;
    }
    spawn_local(async move {
        match wire::verify_paths(paths).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
        rescan_watched(state);
    });
}

/// Measure one address and write the result.
///
/// Called when a book joins the library through the reader rather than through an
/// import: an open proves the file is there and measures nothing, so the row it
/// leaves behind carries a placeholder identity — and a placeholder is exactly
/// what [`rescan_watched`] refuses to diff against. One file's metadata is a
/// cheap way to keep a hand-opened book from holding every watched folder off
/// until the next launch.
pub fn verify_one(state: AppState, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        match wire::verify_paths(vec![path]).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
    });
}

/// Write a batch of path checks into the library. Split out of
/// [`verify_library`] because a relink asks for exactly the same thing about one
/// address, and one definition of "what a measurement does to a book" is one
/// fewer place for the two to disagree.
fn apply_checks(state: AppState, checks: &[PathCheck]) {
    let mut changed = false;
    state.library.books.update(|rows| {
        for check in checks {
            if !apply_check(rows, check).is_empty() {
                changed = true;
            }
        }
    });
    if !changed {
        return;
    }
    crate::storage::persist_library(state.library);
}

// ---------------------------------------------------------------------------
// The import itself.
// ---------------------------------------------------------------------------

// A book about to be placed is a `(id, found file)` pair, and the id is minted
// BEFORE any copy happens, because the stored file is named after it: minting
// afterwards would leave two imports of a folder that holds a `report.pdf`
// fighting over one `report_0.pdf` in the store.

/// Scan one folder, run the ledger over what the walk found, copy whatever the
/// options say to copy, and write the result in one go.
async fn run_folder(
    state: AppState,
    task: String,
    root: String,
    opts: FolderOpts,
    asked: Asked,
) {
    // A rescan is the quiet half of this function: it owes the reader no card
    // and no write for a folder nothing changed in. An import owes an answer
    // either way.
    let quiet = asked == Asked::OnFocus;
    let found = match wire::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, quiet),
    };

    // A snapshot, and only a snapshot: the ledger needs a consistent library to
    // diff against, but nothing below writes these copies back. What lands is
    // applied to the live signals at the end, so a book opened while the walk was
    // running is not overwritten by one.
    let mut books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();

    // The folder's ledger row. Importing a folder the library already watches
    // continues that row's `placed` and `ignored` sets — which is the whole point
    // of them: re-importing is how a reader would otherwise get back every book
    // they deleted last week.
    let mut folder = folders
        .iter()
        .find(|f| f.root == root)
        .cloned()
        .unwrap_or_else(|| WatchedFolder {
            id: id::next_folder_id(crate::storage::now_ms()),
            root: root.clone(),
            opts: opts.clone(),
            placed: HashSet::new(),
            ignored: Vec::new(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        });
    // The sheet's answers are this import's truth, and the next scan's.
    folder.opts = opts;

    // The tree on disk is the tree on the shelf, for the shelves this folder
    // owns: a folder card cut from a watched tree is a VIEW of that tree, so its
    // rung is the one its `rel` names — including for shelves an older, flatter
    // build minted as siblings, which this pass re-hangs under the rung they were
    // always cut from. Virtual shelves are the reader's own arrangement and are
    // never touched here, and neither is a shelf of another folder — nor a shelf
    // the reader moved BY HAND, which `reparent` marked `manual_parent`: the
    // hand beats the disk, and this pass is the disk's. A moved shelf still
    // serves as its subfolders' rung in the map below, so the subtree the reader
    // carried off re-hangs together, wherever it now hangs.
    //
    // Before the diff and before the "nothing changed" return on purpose: a
    // library arranged by an older build is repaired by the first rescan that
    // looks at the folder, not only by an import that happens to add something.
    let rehanged = state.library.shelves.with_untracked(|shelves| {
        let rungs: HashMap<String, String> = shelves
            .iter()
            .filter_map(|s| match &s.kind {
                ShelfKind::Folder { folder_id, rel } if folder_id == &folder.id => {
                    Some((rel.clone().unwrap_or_default(), s.id.clone()))
                }
                _ => None,
            })
            .collect();
        let mut moved = Vec::new();
        for shelf in shelves.iter() {
            // The reader's placement wins over the disk's shape.
            if shelf.manual_parent {
                continue;
            }
            let want: Option<Option<String>> = match &shelf.kind {
                ShelfKind::Folder { folder_id, rel } if folder_id == &folder.id => {
                    let key = rel.clone().unwrap_or_default();
                    Some(parent_key(&key).and_then(|rung| rungs.get(rung).cloned()))
                }
                _ => None,
            };
            if let Some(want) = want {
                // A shelf is never its own parent, whatever a stale map claims.
                let want = want.filter(|w| w != &shelf.id);
                if shelf.parent != want {
                    moved.push((shelf.id.clone(), want));
                }
            }
        }
        moved
    });
    if !rehanged.is_empty() {
        state.library.shelves.update(|shelves| {
            for (id, want) in &rehanged {
                if let Some(shelf) = shelves.iter_mut().find(|s| &s.id == id) {
                    shelf.parent = want.clone();
                }
            }
        });
        crate::storage::persist_library(state.library);
    }

    let registry = ledger::registry_of(&books);

    // A fingerprint can rejoin the library by any route — a hand-open, a second
    // folder's import, a restore — and a tombstone left behind for a book that
    // exists is a restore row offering something the reader already has.
    ledger::prune_tombstones(&mut folder, &registry);
    // Written on every scan, including one that changes nothing: the restore
    // menu's "did this book move out of my folder" answer is only as fresh as the
    // last walk, and a walk that found nothing to do still saw every file.
    folder.record_seen(&found);

    let mut adds: Vec<FoundFile> = Vec::new();
    let mut relinks: Vec<(String, String)> = Vec::new();
    // Two tables, one question each: what should come back on its own, and what
    // the reader is asking for right now. See `ledger` for which rows differ.
    let actions = match asked {
        Asked::OnFocus => ledger::diff_folder(&folder, &registry, &found),
        Asked::Explicitly => ledger::diff_import(&folder, &registry, &found),
    };
    for action in actions {
        match action {
            ScanAction::Add(file) => adds.push(file),
            ScanAction::Relink { book_id, to } => relinks.push((book_id, to)),
            ScanAction::Skip => {}
        }
    }
    // A relink that would point a book at an address another row already reads
    // is a relink of the WRONG row. Two rows can hold one fingerprint now — a
    // folder imported beside another that held a byte-identical copy — and the
    // registry is first-wins, so it names one of them and a walk of the other
    // folder would rewrite the first one's address out from under it. The
    // address this walk found is already a book's address, so there is nothing
    // here to heal and the walk stays quiet about it.
    relinks.retain(|(_, to)| !book_rows(&books).any(|b| b.path() == to.as_str()));
    let relinked = relinks.len();

    // A file at an address the library already holds IS that book, whatever the
    // two fingerprints say. The case this catches is a row migrated from the
    // previous schema: it carries a placeholder identity because nothing ever
    // measured it, so the ledger above saw "unknown content" — and adding it
    // would put a second copy of the same file on the shelf next to its own
    // twin. Healing the row is the honest answer, and the walk has just made the
    // measurement the startup pass could not.
    let mut healed = 0usize;
    adds.retain(|file| match book_rows_mut(&mut books).find(|b| b.path() == file.path) {
        Some(book) => {
            book.fp = file.fp;
            book.fp_pending = false;
            book.missing = false;
            healed += 1;
            false
        }
        None => true,
    });

    // One book per fingerprint, inside a single scan as well as across scans: a
    // tree holding two byte-identical files is one book, and copying both would
    // leave an orphan in the store that nothing can ever remove.
    // One book per fingerprint INSIDE a single scan, always: a tree holding two
    // byte-identical files is one book, and copying both would leave an orphan
    // in the store that nothing can ever remove. Across scans it is the
    // RESCAN's rule and not an explicit import's — a reader who asks for this
    // folder is asking for the files in it, and a byte-identical copy of a book
    // another folder placed is still a file this folder holds, so it is still a
    // book on this folder's shelf.
    let mut seen: HashSet<Fingerprint> = match asked {
        Asked::OnFocus => registry.keys().copied().collect(),
        Asked::Explicitly => HashSet::new(),
    };
    adds.retain(|f| seen.insert(f.fp));

    if adds.is_empty() && relinked == 0 && healed == 0 {
        // Nothing to do. A quiet run leaves no trace beyond the folder's own
        // "last scanned" stamp; an explicit import still owes the reader an
        // answer, which is a card saying nothing was new.
        folder.scanned_ms = crate::storage::now_ms();
        write_folder(state, folder);
        if !quiet {
            update_task(state, &task, |t| t.finish());
        }
        return;
    }

    let now = crate::storage::now_ms();
    // Minted off the crate's own counter rather than the snapshot's length: two
    // watched folders rescan concurrently, and two tasks that both counted the
    // library as it was BEFORE their walks would mint the same id twice in the
    // same millisecond — two books wearing one id, which the next load's
    // sanitize resolves by dropping one of them.
    let pending: Vec<(String, &FoundFile)> = adds
        .iter()
        .map(|file| (id::next_id(now), file))
        .collect();
    let expected = (pending.len() + relinked + healed) as u32;
    if quiet {
        // The first card appears only now, so a focus rescan that found nothing
        // never raises one at all.
        let mut card = ImportTask::new(task.clone(), folder_label(&root));
        card.total = expected;
        push_task(state, card);
    } else {
        update_task(state, &task, move |t| t.total = expected);
    }

    let copies = if folder.opts.in_place {
        HashMap::new()
    } else {
        match copy_batch(state, &task, &pending).await {
            Ok(copies) => copies,
            Err(message) => return fail(state, &task, message, quiet),
        }
    };

    // Applied to the LIVE lists rather than to the copies taken before the scan.
    // A walk of a big folder takes seconds, and a reader who opens a book during
    // one would otherwise have that read overwritten by the write at the end — or,
    // if the book was new to the library, dropped from it entirely. `update`
    // re-reads inside the write, so an import lands on top of whatever happened
    // while it was walking.
    let mut placed = 0u32;
    let mut relink_count = 0usize;
    let mut new_shelves: Vec<Shelf> = Vec::new();
    let mut placements: Vec<(String, String)> = Vec::new();
    let in_place = folder.opts.in_place;
    let folder_id = folder.id.clone();

    state.library.books.update(|books| {
        for (book_id, to) in relinks {
            if ledger::relink(books, &book_id, &to) {
                relink_count += 1;
            }
        }
        for (book_id, file) in pending {
            // Second half of the heal-by-address rule, for the book that appeared
            // while the walk was running: an address the library now holds is that
            // book, so it is measured rather than added a second time beside its
            // twin. Membership is left alone — the ledger records the placement,
            // and where the reader filed it is the reader's business.
            if let Some(existing) = book_rows_mut(books).find(|b| b.path() == file.path) {
                existing.fp = file.fp;
                existing.fp_pending = false;
                existing.missing = false;
                folder.mark_placed(file.fp);
                healed += 1;
                continue;
            }
            let origin = if in_place {
                Origin::Linked {
                    src: file.path.clone(),
                }
            } else {
                let Some(store) = copies.get(&book_id) else {
                    // The copy failed, so there are no bytes to read: a book here
                    // would be a card that opens onto an error. The failure is
                    // already on the toast the batch raised.
                    continue;
                };
                Origin::Stored {
                    src: Some(file.path.clone()),
                    store: store.clone(),
                }
            };
            // Measured by the shell's walk, so this is a real fingerprint and
            // not a placeholder: nothing about this book is pending.
            let book = Book::new(
                book_id,
                file.fp,
                file.format().unwrap_or(Format::Pdf),
                origin,
                now,
            );
            let placed_id = add_book(books, book);
            let key = folder.shelf_key(file);
            // The whole chain, not the leaf: importing "1" whose inside is "2",
            // "3" and four books has to produce "1" at the root with "2", "3" and
            // the four books inside it — not three siblings at the root and the
            // books twice. `shelf_chain_for` mints every rung between the root
            // shelf and the file's own subfolder, and reuses the rungs a previous
            // scan already minted.
            let shelf_id = folder.shelf_chain_for(
                &key,
                |_| id::next_shelf_id(now),
                |rung| shelf_name(rung, &root),
                |rung, id, name, parent| {
                    new_shelves.push(Shelf {
                        id: id.to_string(),
                        name,
                        kind: ShelfKind::Folder {
                            folder_id: folder_id.clone(),
                            rel: rel_of(rung),
                        },
                        books: Vec::new(),
                        parent,
                        // Minted by the scan, so the scan owns its rung —
                        // until a hand moves it, which is `reparent`'s mark.
                        manual_parent: false,
                    });
                },
            );
            placements.push((placed_id, shelf_id));
            folder.mark_placed(file.fp);
            // The book landed, so a removal that was holding it out is spent.
            // Lifted here rather than with the diff: a copy that fails leaves
            // the tombstone standing, which is the one honest outcome for a
            // file that could not be filed.
            ledger::restore_deleted(&mut folder, &file.fp);
            placed += 1;
        }
    });

    state.library.shelves.update(|shelves| {
        for shelf in new_shelves {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        for (book_id, shelf_id) in &placements {
            let Some(shelf) = shelves.iter_mut().find(|s| &s.id == shelf_id) else {
                continue;
            };
            // Two byte-identical files in one tree are one book, so the second
            // resolves to an id that is already a member: appending it again would
            // reshuffle the shelf the reader can see.
            if !shelf.books.iter().any(|m| m == book_id) {
                shelves_ops::place(&mut shelf.books, book_id, None);
            }
        }
    });

    folder.scanned_ms = now;
    write_folder(state, folder);
    crate::storage::persist_library(state.library);
    // A folder import is the case this matters most: it is the one way a shelf
    // arrives with dozens of books at once, and a plate of fallbacks is not a
    // shelf the reader can scan.
    super::covers::backfill_missing(state);

    let total = placed + (relink_count + healed) as u32;
    update_task(state, &task, move |t| {
        t.total = total;
        t.done = total;
        t.finish();
    });
}

/// Put a removed book back, from the folder's own import menu.
///
/// Not a rescan with the tombstone lifted: an explicit restore is an explicit
/// choice, so it honours the folder's read-in-place-or-copy answer and ignores the
/// format and size filters that a passive scan applies — a reader who removed a
/// 12 KB text file and then asks for it back is not asking to be told it is too
/// small. The file is measured first, and a measurement that comes back empty
/// leaves the tombstone exactly where it was, because losing it would lose the
/// only record the book was ever there.
pub fn restore_deleted_book(state: AppState, folder_id: String, fp: Fingerprint) {
    // The folder's options decide read-in-place-or-copy; its root does not appear,
    // because a restore measures the address the tombstone recorded rather than
    // assuming the file is still where the folder put it.
    let taken = state.library.folders.with_untracked(|folders| {
        folders.iter().find(|f| f.id == folder_id).and_then(|f| {
            ledger::find_tombstone(f, &fp)
                .map(|entry| (f.opts.clone(), entry.clone()))
        })
    });
    let Some((opts, entry)) = taken else {
        return;
    };

    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), entry.label()));
    spawn_local(async move {
        let checks = match wire::verify_paths(vec![entry.last_path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return fail(state, &task, message, false),
        };
        let Some(found) = checks.first().and_then(found_from_check) else {
            return fail(
                state,
                &task,
                format!("{} is not there any more.", entry.label()),
                false,
            );
        };

        let now = crate::storage::now_ms();
        let book_id = id::next_id(now);
        let origin = if opts.in_place {
            Origin::Linked {
                src: found.path.clone(),
            }
        } else {
            let requests = [StoreRequest {
                path: found.path.clone(),
                id: book_id.clone(),
            }];
            match wire::store_books(&task, &requests).await {
                Ok(results) => match results.into_iter().next() {
                    Some(result) if result.is_ok() => Origin::Stored {
                        src: Some(found.path.clone()),
                        store: result.store,
                    },
                    Some(result) => {
                        let message = result
                            .error
                            .unwrap_or_else(|| "Could not copy that file.".to_string());
                        return fail(state, &task, message, false);
                    }
                    None => {
                        return fail(state, &task, "Could not copy that file.".to_string(), false)
                    }
                },
                Err(message) => return fail(state, &task, message, false),
            }
        };

        let book = Book {
            // The name the shelf showed before the removal, so a restored book
            // comes back as the book the reader remembers rather than as a
            // file stem.
            title: entry.title.clone(),
            // The file's fingerprint as it is NOW, which is not necessarily the
            // one the tombstone carries: a book can be edited between being
            // removed and being asked for back.
            ..Book::new(
                book_id,
                found.fp,
                found.format().unwrap_or(Format::Pdf),
                origin,
                now,
            )
        };
        let mut placed_id = String::new();
        state.library.books.update(|books| {
            placed_id = add_book(books, book);
        });

        // The removal comes out only now that the book is back, and `placed` goes
        // in at the same moment: a fingerprint the ledger skips with no book
        // behind it is the one state a folder cannot recover from on its own.
        let stale = entry.fp;
        state.library.folders.update(|folders| {
            let Some(folder) = folders.iter_mut().find(|f| f.id == folder_id) else {
                return;
            };
            ledger::restore_deleted(folder, &stale);
            folder.mark_placed(found.fp);
            if found.fp != stale {
                // The file changed while it was gone, so the old fingerprint's
                // tombstone describes a file that no longer exists. Drop it rather
                // than leave a restore row that measures nothing.
                folder.ignored.retain(|t| t.fp != found.fp);
            }
        });

        // Back on the shelf it was filed on, or on the folder's root shelf if that
        // shelf has since gone: a restored book should not come back somewhere new.
        state.library.shelves.update(|shelves| {
            let known = entry
                .shelf_id
                .as_deref()
                .filter(|id| shelves.iter().any(|s| s.id == **id));
            let target = known
                .map(str::to_string)
                .or_else(|| root_shelf_of(shelves, &folder_id));
            let Some(shelf_id) = target else {
                return;
            };
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
                shelves_ops::shelf_add(shelf, &placed_id);
            }
        });
        crate::storage::persist_library(state.library);
        // A removed book took its cover with it; a restored one gets it back
        // without asking to be opened first.
        super::covers::backfill_missing(state);
        update_task(state, &task, |t| {
            t.total = 1;
            t.done = 1;
            t.finish();
        });
    });
}

/// The shelf a folder's root files onto, if it has one.
fn root_shelf_of(shelves: &[Shelf], folder_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| s.kind.is_folder_root() && s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}

/// Copy one batch into the store, answering with the stored address per book id.
///
/// A per-file failure is collected rather than fatal: the reader gets every book
/// that copied, plus one toast naming the ones that did not.
async fn copy_batch(
    state: AppState,
    task: &str,
    pending: &[(String, &FoundFile)],
) -> Result<HashMap<String, String>, String> {
    let requests: Vec<StoreRequest> = pending
        .iter()
        .map(|(book_id, file)| StoreRequest {
            path: file.path.clone(),
            id: book_id.clone(),
        })
        .collect();
    let results = wire::store_books(task, &requests).await?;
    let mut copies = HashMap::new();
    let mut failures = Vec::new();
    for result in results {
        if result.is_ok() {
            copies.insert(result.id, result.store);
        } else {
            failures.push(file_name(&result.src));
        }
    }
    if !failures.is_empty() {
        let message = match failures.len() {
            1 => format!("Could not copy {}", failures[0]),
            n => format!("Could not copy {n} files, starting with {}", failures[0]),
        };
        state.toast(message);
    }
    Ok(copies)
}

/// Import loose files: measure them, then file them — except the ones whose
/// content the target level already holds, which ask (see
/// [`crate::services::library::conflict`]).
async fn run_files(state: AppState, task: String, paths: Vec<String>, target: Option<String>) {
    let checks = match wire::verify_paths(paths).await {
        Ok(checks) => checks,
        Err(message) => return fail(state, &task, message, false),
    };
    let found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
    if found.is_empty() {
        return fail(
            state,
            &task,
            "None of those files could be opened.".to_string(),
            false,
        );
    }
    // Measuring a file the library already holds refreshes its fingerprint and
    // clears a migrated book's pending mark; it has to land before the adds
    // below, which dedupe against exactly those fingerprints.
    apply_checks(state, &checks);

    // Every file whose NAME the target level already holds is a question
    // rather than a placement — the old rule resolved the file to the row the
    // library already had and skipped the placement, which to the reader was a
    // book swallowed by the shelf it was dropped on. The question is the level's
    // and is about a name, so a file whose twin sits on another shelf is not
    // one: the row the library already has simply gains this level as well,
    // which is a landing the reader can see and the one they asked for by
    // dropping here.
    let shelf_id = target
        .clone()
        .unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());
    let arrivals: Vec<Arrival> = found
        .iter()
        .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
        .collect();
    let (clean, conflicts) = conflict::screen(state, arrivals);
    let placed = clean.len() as u32;
    let waiting = conflicts.len() as u32;
    // One persist for the batch rather than one per file: a drop of four
    // hundred files is one write, and a reader who closes the window halfway
    // through an import should find all of it or none of it.
    for arrival in &clean {
        let Some(file) = arrival.file.as_ref() else {
            continue;
        };
        land_file(state, file, None, &arrival.shelf_id, arrival.index);
    }
    // Raised after the clean half landed: the sheet counts the questions, and
    // a landing that shifted a member list is one the answers resolve against.
    conflict::raise(state, conflicts);
    crate::storage::persist_library(state.library);
    update_task(state, &task, move |t| {
        t.total = placed;
        t.done = placed;
        t.waiting = waiting;
        t.finish();
    });
}

/// Land one measured file as a book row on one level.
///
/// The silent half of an import, and the half an answer to the collision sheet
/// calls once it knows the name to use. `name` is the title the row is given:
/// `None` leaves it without one, so the stem of its address is the name the
/// shelf shows, which is the honest name for a file nobody has opened yet, and
/// `Some` is the counter the sheet minted for a second book of one name.
///
/// A second row of an address the library already reads is a book of its own
/// ([`Book::independent`]): its highlights live under a key carrying its id and
/// its resume point is written by itself, which is what makes "add as new"
/// mean something a reader can see rather than a second name for one book.
///
/// The row is always a new one, named or not. The name is the sheet's *add as
/// new* and the absence of one is a file the level had no book of that name
/// for; either way the reader asked for a book HERE, and a resolve to the row
/// another level holds would answer with nothing new on the level they dropped
/// on — the vanishing the collision sheet exists to stop.
///
/// Writes no persist, because a caller that lands four hundred files owes one
/// write and only the caller knows whether this is one file or four hundred.
pub fn land_file(
    state: AppState,
    file: &FoundFile,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    let now = crate::storage::now_ms();
    let independent = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.path() == file.path));
    let book = Book {
        title: name,
        independent,
        ..Book::new(
            id::next_id(now),
            file.fp,
            file.format().unwrap_or(Format::Pdf),
            Origin::Linked {
                src: file.path.clone(),
            },
            now,
        )
    };
    // Always a row of its own, and never a resolve to the row the library
    // already holds: the reader asked for THIS file on THIS level, and filing
    // another level's row here leaves the level they dropped on with a book
    // that is not its own — one removal from both shelves, one resume point
    // between them, and a shelf that shows a book the reader never put there.
    // Content identity is the ledger's business, which is a rescan's and a
    // watched folder's; it is not an answer to a hand. What keeps two rows of
    // one file honest is the mark above, not a dedupe here.
    let placed = book.id.clone();
    state.library.books.update(|rows| rows.push(Row::Book(book)));
    if shelf_id != shelves_ops::ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
                shelves_ops::place(&mut shelf.books, &placed, index);
            }
        });
    }
    // The row may read from an address the cache has no art for, and the queue
    // is the shelf's one answer to that.
    super::covers::backfill_missing(state);
    placed
}

/// A path check turned into a found file, so loose files and a folder walk feed
/// the same placement code. `None` for an address that did not resolve, or one
/// the format registry does not know.
fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
    if !is_supported_path(&check.path) {
        return None;
    }
    let fp = check.fingerprint()?;
    let name = file_name(&check.path);
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default();
    Some(FoundFile {
        rel: name,
        path: check.path.clone(),
        ext,
        size: check.size,
        fp,
    })
}

/// Put the folder row back. One place, because the ledger is the part of the
/// library that must never be written half-updated: a `placed` set that lost an
/// entry re-adds a book the reader already filed. Written through `update` for the
/// same reason the books are — two imports running at once must not each replace
/// the other's folder row.
fn write_folder(state: AppState, folder: WatchedFolder) {
    state.library.folders.update(|folders| {
        match folders.iter().position(|f| f.id == folder.id) {
            Some(at) => folders[at] = folder,
            None => folders.push(folder),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{claim_root, land_file, rel_of, shelf_name};
    use crate::state::AppState;
    use leptos::prelude::*;
    use library_core::book::Fingerprint;
    use library_core::scan::FoundFile;
    use library_core::shelf::Shelf;

    /// A measured Markdown file: the cover queue skips anything that is not a
    /// PDF, so a host test that lands one never starts the wasm render chain.
    fn found(path: &str, n: u32) -> FoundFile {
        FoundFile {
            rel: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            ext: "md".to_string(),
            size: u64::from(n),
            fp: Fingerprint {
                size: u64::from(n),
                mtime_ms: u64::from(n),
                head_hash: n,
            },
        }
    }

    fn plain(id: &str) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: Default::default(),
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }
    }

    #[test]
    fn a_file_lands_as_its_own_row_on_the_level_it_was_dropped_on() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.shelves.set(vec![plain("a"), plain("b")]);
        let file = found("/one/notes.md", 7);

        land_file(state, &file, None, "a", None);
        assert_eq!(state.library.books.get_untracked().len(), 1);

        // The same file, imported onto an unrelated level. Nothing on that
        // level holds the name, so nothing asks — and the answer to nothing
        // asking is a book on that level, not a shrug. Filing the first
        // level's row here instead would leave the reader looking at a shelf
        // that gained nothing they put there, and one removal would take the
        // book off both.
        land_file(state, &file, None, "b", None);
        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 2, "each level gets a book of its own");
        assert!(
            rows[1].book().is_some_and(|b| b.independent),
            "two books of one address keep their own highlights and place"
        );
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves.iter().find(|s| s.id == "b").map(|s| s.books.len()),
            Some(1),
            "and the level it was dropped on is the level it landed on"
        );

        // A name the sheet minted always makes a row too, whatever the library
        // holds: "add as new" is an instruction to add a book.
        land_file(
            state,
            &found("/two/other.md", 9),
            Some("other_1".into()),
            "b",
            None,
        );
        assert_eq!(state.library.books.get_untracked().len(), 3);
    }

    #[test]
    fn one_root_is_one_run_at_a_time() {
        let first = claim_root("/books");
        assert!(first.is_some());
        assert!(
            claim_root("/books").is_none(),
            "a second walk of the same tree is refused while the first is live"
        );
        assert!(
            claim_root("/other").is_some(),
            "a different folder is a different run"
        );
        drop(first);
        assert!(
            claim_root("/books").is_some(),
            "and the release is the run ending, whatever ended it"
        );
    }

    #[test]
    fn a_shelf_is_called_by_its_subfolder_and_the_root_by_its_folder() {
        assert_eq!(shelf_name("scifi", "/Users/me/Books"), "scifi");
        assert_eq!(shelf_name("scifi/deep", "/Users/me/Books"), "deep");
        assert_eq!(shelf_name("", "/Users/me/Books"), "Books");
    }

    #[test]
    fn only_the_root_shelf_has_no_subfolder() {
        assert_eq!(rel_of(""), None);
        assert_eq!(rel_of("scifi").as_deref(), Some("scifi"));
        assert_eq!(rel_of("scifi/deep").as_deref(), Some("scifi/deep"));
    }
}
