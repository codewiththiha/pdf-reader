//! Importing: running the shell's measurements through the library's ledger and
//! writing the answer to the state.
//!
//! One path for all three ways books arrive — the folder sheet, a handful of
//! files from the picker or a drop, and a rescan of a watched folder when the
//! window regains focus. They differ in where the measurements come from and
//! nothing else, so the deciding and the writing happen once, here.
//!
//! Four rules this module exists to keep:
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
    Book, Fingerprint, Origin, Row, add_book, apply_check, book_rows, book_rows_mut, find_book_mut,
    find_by_id, find_row,
};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops, FolderOpts, Tombstone, WatchedFolder};
use library_core::id;
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};
use library_core::wire::{PathCheck, StoreRequest, StoreResult};
use reader_core::format::{Format, is_supported_path};

use super::arrange::{PurgeOpts, migrate_gloss};
use super::conflict::{self, ConflictAsk};
use super::{file_name, folder_label};
use crate::services::library as wire;
use crate::time::now_ms;
use crate::state::library::{ImportTask, NoteKind};
use crate::state::{AppState, Toast};

/// Who asked for a folder run, which is what the tombstones mean.
///
/// One type rather than a boolean at the call site because the two runs are not
/// two settings of one thing: they answer different questions, and a boolean at
/// `run_folder`'s signature cannot say which one it was answering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// The reader picked the folder, or dropped it on the window. An explicit
    /// ask overrides the tombstones their own earlier removals wrote.
    Explicitly,
    /// The window regaining focus asked. This is exactly the case a tombstone
    /// exists for — a removed book must stay removed on its own — so they hold.
    OnFocus,
}

/// What the folder sheet's answer decided about the run's root, before the run.
///
/// The default is the run nobody asked about: mint the folder's root shelf
/// under the folder's own name. A collision at the level changes that, and
/// the change is a value rather than a branch at six call sites:
///
///   * `rename` — the *as new* answer's name: the root rung is minted under
///     the counter the sheet promised rather than under the folder's own;
///   * `into` — the *merge* answer's shelf: the folder's root rung IS the
///     shelf the level already held, and every file the walk finds at the
///     root files into it;
///   * `continuation` — the root shelf an already-imported read-at-place
///     re-pick names, and the note a walk that found NOTHING new owes the
///     reader after the fact: the reconciliation ran, every book was already
///     here, and the shelf lights up when the note closes. A run that found
///     something answers with the landing and the card instead, and a rescan
///     never carries one.
#[derive(Clone, Default)]
pub(crate) struct RootPlan {
    pub rename: Option<String>,
    pub into: Option<String>,
    pub continuation: Option<(String, String)>,
}

/// A run's id. The shell echoes it on every progress beat, so two imports in
/// flight never mix their counts, and the dock can look a card up by it.
fn task_id() -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    format!(
        "t{:x}-{}",
        now_ms(),
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

/// The sentence a second ask for a folder that is already being walked gets.
///
/// One spelling, because the two doors that can refuse a run — an import and the
/// mode switch's replace — refuse it for the same reason and owe the reader the
/// same words. A toast each door worded itself would eventually differ about
/// whether the refusal was about this folder or about imports generally.
fn already_importing(state: AppState, root: &str) {
    state.ui.toast.set(Some(Toast::new(format!(
        "{} is already being imported.",
        folder_label(root)
    ))));
}

/// Whether a walk of `root` is already in flight — the question the replace
/// answer has to ask BEFORE its purge, because a removal behind a refused
/// claim would be a sweep with no import to answer it.
fn root_is_claimed(root: &str) -> bool {
    RUNNING.with(|running| running.borrow().contains(root))
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

/// The shelf a found file belongs on: every rung between the folder's root shelf
/// and the file's own subfolder, minted or reused, with each rung this call minted
/// collected into `new_shelves` for the caller to put a shelf row under.
///
/// One spelling for the two loops a folder run mints through — the books it adds
/// and the books the library already held, which a planned tree owes a membership
/// of — because the two have to agree about what a rung is CALLED and about who
/// OWNS it, and a second copy was a second answer to both. An *as new* run that
/// named its root rung one way for new files and another for known ones would
/// have minted two trees side by side instead of one.
///
/// The whole chain rather than the leaf, which is `shelf_chain_for`'s own rule:
/// importing "1" whose inside is "2", "3" and four books has to produce "1" at the
/// root with "2", "3" and the four books inside it — not three siblings at the
/// root and the books twice.
fn chain_for(
    folder: &mut WatchedFolder,
    key: &str,
    now: u64,
    root: &str,
    planned_name: &Option<String>,
    merged: bool,
    new_shelves: &mut Vec<Shelf>,
) -> String {
    let folder_id = folder.id.clone();
    folder.shelf_chain_for(
        key,
        |_| id::next_shelf_id(now),
        |rung| match (rung.is_empty(), planned_name) {
            // The folder sheet's *as new* answer: the root rung wears the counter
            // name it promised, and every rung below it keeps the disk's own.
            (true, Some(name)) => name.clone(),
            _ => shelf_name(rung, root),
        },
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
                // Minted by the scan, so the scan owns its rung — until a hand
                // moves it, which is `reparent`'s mark. A merged run marks every
                // rung it mints: their tree hangs off a shelf the disk does not
                // own, so there is no disk shape for a re-hang to put them back on.
                manual_parent: merged,
            });
        },
    )
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

/// Close a dock card on its final counts.
///
/// One spelling for the runs that finish one — a folder walk, a loose-file drop
/// and a restore — because a card is a report and three routes into the library
/// reporting three different sets of numbers is three answers about one import.
fn finish_task(state: AppState, task: &str, total: u32, waiting: u32) {
    update_task(state, task, move |t| {
        t.total = total;
        t.done = total;
        t.waiting = waiting;
        t.finish();
    });
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
    state.ui.toast.set(Some(Toast::new(toast)));
}

// ---------------------------------------------------------------------------
// The three ways in.
// ---------------------------------------------------------------------------

/// Import a folder, with the options the sheet was filled in with. Returns
/// immediately: the dock owns the feedback from here on.
/// A folder whose NAME the root level already holds is a question before it
/// is an import — two shelves of one name on one level are two doors a reader
/// cannot tell apart, which is the shelf's own spelling of the collision the
/// book sheet asks about. The question goes to the folder sheet
/// (`crate::services::library::conflict`), and the run starts with the answer
/// it gave as its [`RootPlan`].
///
/// ALWAYS: a reader who picked a folder and clicked Import asked for an
/// answer, and a run that ends on "Imported 0 books" with no sheet in between
/// is the silent nothing the book collision used to be. A folder colliding
/// with its OWN previous shelf asks too — the continuation is a choice rather
/// than a surprise.
///
/// The read-at-place gate in front of all of it is three answers rather than
/// one (`covered_shelf`). A RUNG inside a tree the library reads in place —
/// the folder itself or a subfolder of it — cannot mint a second instance,
/// so it is answered before any sheet with "already imported" and a
/// highlight of the shelf the reader meant. The tree's OWN root, re-picked
/// read at its place, is a reconciliation instead: the walk runs, new files
/// join the tree as linked books, the logs a removal or a departure wrote
/// are spent by their books coming back, and only a walk that found NOTHING
/// raises the note — the sentence the gate used to say up front, earned by
/// the walk instead. The same root re-picked as COPIES is the mode switch's
/// question, and the sheet it raises asks how the folder is held from here
/// on: a second shelf of copies, the shelf that is here switched over to
/// copies, or copies in place of the books that are here. The ordinary name
/// sheet, meanwhile, withholds *as new* from a read-at-place arrival of a
/// DIFFERENT folder's name, because as new of a linked folder is exactly the
/// second instance the gate exists to prevent; a stored arrival keeps all
/// three answers, its copies being the library's own.
pub fn import_folder(state: AppState, root: String, opts: FolderOpts) {
    // The read-at-place gate, which is three answers rather than one. A RUNG
    // inside a tree the library reads in place is ground that tree already
    // holds: a sentence and a highlight, whichever mode arrives, because a
    // second instance of it is a second door on one folder. The tree's OWN
    // root re-picked is a continuation instead: read at its place, the run
    // reconciles — new files join the tree, the logs a removal or a
    // departure wrote are spent by their books coming back, and only a walk
    // that found nothing raises the note. Re-picked as COPIES, the folder is
    // asking to be held the other way, and that is the mode switch's
    // question: a second shelf of copies, the shelf that is here switched
    // over to copies, or copies in place of the books that are here.
    if let Some((rel, shelf_id, shelf_name)) = covered_shelf(state, &root) {
        if !rel.is_empty() {
            conflict::raise_note(state, shelf_id, shelf_name, NoteKind::Gated);
            return;
        }
        if opts.in_place {
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    continuation: Some((shelf_id, shelf_name)),
                    ..Default::default()
                },
            );
            return;
        }
        conflict::raise_shelf(
            state,
            conflict::ShelfConflictAsk {
                incoming_name: folder_label(&root),
                existing_id: shelf_id,
                existing_name: shelf_name,
                root,
                opts,
                own: true,
                mode_switch: true,
            },
        );
        return;
    }
    let incoming = folder_label(&root);
    let shelves = state.library.shelves.get_untracked();
    if let Some(existing_id) = library_core::conflict::collide_shelf(&shelves, None, &incoming) {
        // The folder's own root shelf, when a previous run minted one: the
        // `shelf_map`'s root rung is the whole of that memory.
        let own = state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .find(|f| f.root == root)
                .and_then(|f| f.shelf_map.get("").cloned())
                == Some(existing_id.clone())
        });
        let existing_name = shelves_ops::find(&shelves, &existing_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| incoming.clone());
        conflict::raise_shelf(
            state,
            conflict::ShelfConflictAsk {
                incoming_name: incoming,
                existing_id,
                existing_name,
                root,
                opts,
                own,
                mode_switch: false,
            },
        );
        return;
    }
    proceed_folder(state, root, opts, RootPlan::default());
}

/// The standing shelf an in-place tree already holds for `root`, if any, as
/// `(rel, shelf id, shelf name)`: `rel` empty when `root` IS a folder the
/// library reads in place, the rung's key when `root` is a subfolder inside
/// one — and the empty key wins when two in-place trees nest, because a
/// folder's own tree answers for it before a tree it stands inside.
///
/// The gate answers only for READ-AT-PLACE trees: their shelves are the OS
/// folders themselves, so a second import of the same ground is at best a
/// no-op and at worst a duplicate of every book on it. Stored trees are
/// never covered — a stored import is the library's own copy, and whether to
/// make another is the reader's call, asked through the ordinary name
/// question.
fn covered_shelf(state: AppState, root: &str) -> Option<(String, String, String)> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let mut rung: Option<(String, String, String)> = None;
    for folder in folders.iter().filter(|f| f.opts.in_place) {
        let Some(rel) = rel_under(root, &folder.root) else {
            continue;
        };
        let Some(shelf_id) = folder.shelf_map.get(&rel) else {
            continue;
        };
        if let Some(shelf) = shelves_ops::find(&shelves, shelf_id) {
            if rel.is_empty() {
                return Some((rel, shelf.id.clone(), shelf.name.clone()));
            }
            if rung.is_none() {
                rung = Some((rel, shelf.id.clone(), shelf.name.clone()));
            }
        }
    }
    rung
}

/// `root` as a rung key inside `base`'s tree: the empty key when the two are
/// the same folder, the `/`-separated remainder when `root` sits inside
/// `base`, and `None` when it does not. The remainder has to start on a
/// directory edge, so `/books2` is never "inside" `/books`.
fn rel_under(root: &str, base: &str) -> Option<String> {
    fn norm(p: &str) -> String {
        p.trim_end_matches(['/', '\\']).replace('\\', "/")
    }
    let (root, base) = (norm(root), norm(base));
    if root == base {
        return Some(String::new());
    }
    let rest = root.strip_prefix(base.as_str())?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Claim the root and start the run — the half of [`import_folder`] that is
/// the same whatever the folder sheet decided, and the half its answers call
/// directly.
pub(crate) fn proceed_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
) {
    // A folder already being imported is an import already answering this ask:
    // its card is on the dock and its walk is the same tree. Racing it would
    // clobber its ledger write, so the second ask says so instead.
    let Some(claim) = claim_root(&root) else {
        already_importing(state, &root);
        return;
    };
    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), folder_label(&root)));
    spawn_local(async move {
        // Held for the whole run: the drop is the release, on every exit path.
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::Explicitly, plan).await;
    });
}

/// Import files picked from the dialog or dropped on the library.
///
/// The library's own copies: a loose file has no folder to rescan it and no
/// structure to preserve, and a linked row no ledger answers for is a row no
/// rule can keep honest — so the bytes go into the store, the row's identity
/// is the copy's own measurement, and the source address stays provenance.
/// A file an in-place folder's tree already holds is the exception that asks
/// rather than copies: the folder answers for it, and the import becomes the
/// covered question or, when the folder's log remembers the file, the book
/// coming back in its folder's place (see [`run_files`]). `target` is the
/// shelf a drop landed on; `None` files onto no shelf, which leaves the books
/// in "All" and nowhere else — the honest answer for a handful of loose files.
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
            run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
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

/// Measure the rows the walk found at addresses the library already holds,
/// taking those files out of the add list. The heal half of the migrated-row
/// rule: a file at an address the library reads IS that book, whatever the two
/// fingerprints say, and the walk has just made the measurement the startup
/// pass could not. Answers how many rows it healed.
fn heal_by_address(
    books: &mut [Row],
    adds: &mut Vec<FoundFile>,
    skip: &HashSet<String>,
) -> usize {
    let mut healed = 0usize;
    adds.retain(|file| {
        // A mode switch's copy is an add BECAUSE the library holds the
        // address: healing it into the row that is there would answer the
        // second instance the reader asked for with the first one.
        if skip.contains(&file.path) {
            return true;
        }
        match book_rows_mut(books).find(|b| b.path() == file.path) {
            Some(book) => {
                book.heal(file.fp);
                healed += 1;
                false
            }
            None => true,
        }
    });
    healed
}

/// Scan one folder, run the ledger over what the walk found, copy whatever the
/// options say to copy, and write the result in one go.
async fn run_folder(
    state: AppState,
    task: String,
    root: String,
    opts: FolderOpts,
    asked: Asked,
    plan: RootPlan,
) {
    // A rescan is the quiet half of this function: it owes the reader no card
    // and no write for a folder nothing changed in. An import owes an answer
    // either way.
    let quiet = asked == Asked::OnFocus;
    let mut found = match wire::scan_folder(&task, &root, &opts).await {
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
            id: id::next_folder_id(now_ms()),
            root: root.clone(),
            opts: opts.clone(),
            placed: HashSet::new(),
            ignored: Vec::new(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        });
    // The mode switch: a folder the library read in place, re-imported as
    // copies. The plan tells the two shapes apart, because an *as new* run
    // owes its tree copies of its OWN — independent books beside the linked
    // ones the old tree keeps reading — while a merge continues the standing
    // tree and flips those books into the library's copies afterwards. A
    // replace has put them through the removal's sweep before the run even
    // started, so the flip simply finds nothing to do.
    let switching = folder.opts.in_place && !opts.in_place;
    let switch_copies_known = switching && plan.rename.is_some();
    let switch_converts = switching && plan.rename.is_none();
    // The sheet's answers are this import's truth, and the next scan's.
    folder.opts = opts;
    // A merge files into the shelf the level already held: the folder's root
    // rung is that shelf, and the map is the one place the walk, the chain
    // minting and every later rescan read the answer from — which is what
    // makes the merge a promise the next scan keeps.
    if let Some(into) = &plan.into {
        folder.shelf_map.insert(String::new(), into.clone());
    }
    // An *as new* answer owes a tree of its OWN: every rung is minted fresh
    // under the counter-named root rather than reusing the rungs that hang off
    // the shelf the folder used to file onto — and from this run on, the new
    // tree is the folder's tree, which is what the map records.
    if plan.rename.is_some() {
        folder.shelf_map.clear();
    }

    // The tree on disk is the tree on the shelf, for the shelves this folder
    // owns — the rule and its edge cases (a hand-moved shelf keeps its place,
    // a subtree re-hangs together, a virtual shelf is never touched) are
    // `library_core::shelf::rehang_moves`', pure and host-tested; this is the
    // one pass that asks it and applies the answer.
    //
    // Before the diff and before the "nothing changed" return on purpose: a
    // library arranged by an older build is repaired by the first rescan that
    // looks at the folder, not only by an import that happens to add something.
    let rehanged = state
        .library
        .shelves
        .with_untracked(|shelves| shelves_ops::rehang_moves(shelves, &folder.id));
    if !rehanged.is_empty() {
        state.library.shelves.update(|shelves| {
            for (id, want) in &rehanged {
                if let Some(shelf) = shelves_ops::find_mut(shelves, id) {
                    shelf.parent = want.clone();
                }
            }
        });
        crate::storage::persist_library(state.library);
    }

    let registry = ledger::registry_of(&books);

    // The switch's two lists, read off the same snapshot the diff reads and
    // before the run writes anything. The CONVERT list is the merge's: every
    // living linked book the folder's ledger answers for, which the run flips
    // into a copy of the library's own at the end. The COPY list is the *as
    // new* run's: the addresses whose linked book the old tree keeps, and
    // where the new tree lands a copy of its own beside it.
    let convert_ids: Vec<String> = if switch_converts {
        linked_rows_of_placed(&books, &folder.placed)
    } else {
        Vec::new()
    };
    let switch_copy_paths: HashSet<String> = if switch_copies_known {
        found
            .iter()
            .filter(|file| {
                registry.get(&file.fp).is_some_and(|known| {
                    find_by_id(&books, &known.id).is_some_and(|b| {
                        matches!(b.origin, Origin::Linked { .. })
                            && b.path() == file.path
                            && folder.placed.contains(&file.fp)
                    })
                })
            })
            .map(|file| file.path.clone())
            .collect()
    } else {
        HashSet::new()
    };

    // A fingerprint can rejoin the library by any route — a hand-open, a second
    // folder's import, a restore — and a tombstone left behind for a book that
    // exists is a restore row offering something the reader already has.
    ledger::prune_tombstones(&mut folder, &registry);
    // Written on every scan, including one that changes nothing: the restore
    // menu's "did this book move out of my folder" answer is only as fresh as the
    // last walk, and a walk that found nothing to do still saw every file.
    folder.record_seen(&found);

    // A file the folder's log says is REPRESENTED — a moved-out log bound to
    // the stored copy that came home — is an import that succeeds by lighting
    // that row up, not by minting a linked neighbour beside the copy the
    // reader already moved back. The log stays standing: it is the folder's
    // permanent word that this file has a row, and a rescan stays silent
    // about it as it always was. Explicit runs only — a rescan never reveals.
    let mut represented: Vec<String> = Vec::new();
    if !quiet {
        found.retain(|file| {
            let Some(row_id) = ledger::find_tombstone(&folder, &file.fp)
                .and_then(|entry| entry.returned_row.clone())
            else {
                return true;
            };
            let alive = state
                .library
                .books
                .with_untracked(|rows| find_row(rows, &row_id).is_some());
            if alive {
                represented.push(row_id);
                false
            } else {
                // The binding names a dead row: the log is spent of its
                // meaning and the file takes the ordinary import route, which
                // lifts the log when the book lands.
                true
            }
        });
    }

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

    // The ledger answered Skip for the switch's own files — their content is
    // known — but an *as new* copy run owes each of them a book of its own:
    // back onto the add list they go, and the planned-tree pass below keeps
    // them out of the memberships it owes the OTHER known files.
    if switch_copies_known && !switch_copy_paths.is_empty() {
        for file in found
            .iter()
            .filter(|f| switch_copy_paths.contains(&f.path))
        {
            if !adds.iter().any(|a| a.path == file.path) {
                adds.push(file.clone());
            }
        }
    }

    // A planned tree — the folder sheet's *as new* or *merge* answer — owes a
    // placement for EVERY file the walk found that the library already holds:
    // a membership of the row it holds it in, never a second row, because one
    // content is one identity and one identity is one row. Those files are
    // exactly the ones the ledger's table answers with a Skip, so a tree
    // promised as "its own shelf, its own tree" would otherwise hold only the
    // new files — and a re-import of one folder, whose every file is known,
    // would hold nothing at all.
    //
    // A merge asks before it places: a file whose name a STANDING rung holds
    // — the root shelf the answer named, or any subfolder shelf a previous
    // run mapped — is the compact sheet's per-file question, even when the
    // row wearing the name is the row the file resolves to, because a reader
    // who re-imports a folder to reconcile it is owed the three answers per
    // file (one book / replace / as new) wherever the names meet, not a card
    // that says nothing was new. Files no standing name collides with join
    // silently: new books in a merged folder are the default, not a case.
    let mut replacements: Vec<(String, FoundFile)> = Vec::new();
    let mut asks: Vec<ConflictAsk> = Vec::new();
    if plan.rename.is_some() || plan.into.is_some() {
        // Known content never reaches a planned run's add list: its placement
        // is the membership below, and an add would either resolve to the
        // same row twice or — in a copying folder — make a store copy nothing
        // reads. The mode switch's own copies are the exception the rule
        // exists for: a second instance the reader just asked for, of a file
        // the old tree keeps reading in place.
        adds.retain(|f| {
            !registry.contains_key(&f.fp) || switch_copy_paths.contains(&f.path)
        });
        let shelves_now = state.library.shelves.get_untracked();
        for file in &found {
            // The row the library holds this file in: by content identity
            // first (the ledger's own answer), and by address second for the
            // migrated row whose placeholder identity no measurement matched.
            let known = registry
                .get(&file.fp)
                .map(|k| k.id.clone())
                .or_else(|| {
                    book_rows(&books)
                        .find(|b| b.path() == file.path)
                        .map(|b| b.id.clone())
                });
            let Some(row_id) = known else {
                continue;
            };
            // A file the switch copies lands as a book of its own below, not
            // as a membership of the row it duplicates.
            if switch_copy_paths.contains(&file.path) {
                continue;
            }
            let key = folder.shelf_key(file);
            let target = plan.into.as_deref().and_then(|into| {
                if key.is_empty() {
                    Some(into.to_string())
                } else {
                    folder.shelf_map.get(&key).cloned()
                }
            });
            if let Some(target) = target {
                let arrival = Arrival::import(file.clone(), target, None);
                if let Some(existing_id) =
                    library_core::conflict::collide(&books, &shelves_now, &arrival)
                {
                    let existing_name = conflict::existing_name_of(&books, &existing_id, &arrival);
                    asks.push(ConflictAsk::folder_merge(
                        arrival,
                        existing_id,
                        existing_name,
                        folder.opts.in_place,
                        folder.id.clone(),
                    ));
                    continue;
                }
            }
            replacements.push((row_id, file.clone()));
        }
    }

    // A file at an address the library already holds IS that book, whatever the
    // two fingerprints say. The case this catches is a row migrated from the
    // previous schema: it carries a placeholder identity because nothing ever
    // measured it, so the ledger above saw "unknown content" — and adding it
    // would put a second copy of the same file on the shelf next to its own
    // twin. Healing the row is the honest answer, and the walk has just made the
    // measurement the startup pass could not.
    let mut healed = heal_by_address(&mut books, &mut adds, &switch_copy_paths);

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

    // The same question for the files the ledger has NOT seen — the merge's
    // genuinely new arrivals, whose names a standing rung may still hold: a
    // collision here is the compact sheet's too (one book / replace / as new,
    // one at a time or one answer for all). The known files were asked by the
    // planned-tree pass above. Everything else goes in without being asked —
    // new books are the default, not a case. A new file inside a SUBFOLDER a
    // previous run mapped is asked against that subfolder's shelf; a file in
    // a rung being minted fresh has nothing standing to collide with.
    if plan.into.is_some() {
        let shelves_now = state.library.shelves.get_untracked();
        adds.retain(|file| {
            let key = folder.shelf_key(file);
            let target = if key.is_empty() {
                plan.into.clone()
            } else {
                folder.shelf_map.get(&key).cloned()
            };
            let Some(target) = target else {
                return true;
            };
            let arrival = Arrival::import(file.clone(), target, None);
            match library_core::conflict::collide(&books, &shelves_now, &arrival) {
                Some(existing_id) => {
                    let existing_name =
                        conflict::existing_name_of(&books, &existing_id, &arrival);
                    asks.push(ConflictAsk::folder_merge(
                        arrival,
                        existing_id,
                        existing_name,
                        folder.opts.in_place,
                        folder.id.clone(),
                    ));
                    false
                }
                None => true,
            }
        });
    }

    if adds.is_empty()
        && relinked == 0
        && healed == 0
        && asks.is_empty()
        && replacements.is_empty()
        && represented.is_empty()
        && convert_ids.is_empty()
    {
        // Nothing to do. A quiet run leaves no trace beyond the folder's own
        // "last scanned" stamp; an explicit import still owes the reader an
        // answer, which is a card saying nothing was new — and a re-pick of a
        // tree the library already reads in place owes the note as well, the
        // gate's old sentence earned now by a walk that found every book
        // already standing.
        folder.scanned_ms = now_ms();
        write_folder(state, folder);
        if !quiet {
            if let Some((shelf_id, name)) = plan.continuation.clone() {
                conflict::raise_note(state, shelf_id, name, NoteKind::NothingNew);
            }
            update_task(state, &task, |t| t.finish());
        }
        return;
    }

    let now = now_ms();
    // Minted off the crate's own counter rather than the snapshot's length: two
    // watched folders rescan concurrently, and two tasks that both counted the
    // library as it was BEFORE their walks would mint the same id twice in the
    // same millisecond — two books wearing one id, which the next load's
    // sanitize resolves by dropping one of them.
    let pending: Vec<(String, &FoundFile)> = adds
        .iter()
        .map(|file| (id::next_id(now), file))
        .collect();
    let replaced = replacements.len();
    let expected = (pending.len() + relinked + healed + replaced + convert_ids.len()) as u32;
    if quiet {
        // The first card appears only now, so a focus rescan that found nothing
        // never raises one at all.
        let mut card = ImportTask::new(task.clone(), folder_label(&root));
        card.total = expected;
        push_task(state, card);
    } else {
        update_task(state, &task, move |t| t.total = expected);
    }

    let copies = if folder.opts.in_place || pending.is_empty() {
        HashMap::new()
    } else {
        match copy_batch(state, &task, &pending).await {
            Ok(copies) => copies,
            Err(message) => return fail(state, &task, message, quiet),
        }
    };
    // The switch's copies are measured in one pass before a row is promised:
    // an independent copy of a file the old tree still reads must not wear
    // the ORIGINAL's fingerprint — that identity stays the linked book's, and
    // the copy is known by its own bytes, the way every instance the library
    // owns is.
    let switch_measured: HashMap<String, Fingerprint> = if switch_copies_known {
        let stores: Vec<String> = pending
            .iter()
            .filter(|(_, file)| switch_copy_paths.contains(&file.path))
            .filter_map(|(book_id, _)| copies.get(book_id).cloned())
            .collect();
        measure_stores(stores).await
    } else {
        HashMap::new()
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
    // Books that landed over a removal this folder remembered: the run
    // reveals the first of them at the end, because "it came back" is worth
    // one highlight and no sentence.
    let mut restored: Vec<String> = Vec::new();
    let in_place = folder.opts.in_place;
    let planned_name = plan.rename.clone();
    let merged = plan.into.is_some();

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
            // and where the reader filed it is the reader's business. The mode
            // switch's copies are exempt by name: their twin IS the point of
            // them, and the run below mints them past the one-row rule.
            let switch_copy = switch_copy_paths.contains(&file.path);
            if !switch_copy
                && let Some(existing) = book_rows_mut(books).find(|b| b.path() == file.path)
            {
                existing.heal(file.fp);
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
            //
            // A file this folder remembers REMOVING is coming back on an
            // explicit ask, and the reader should not notice it was ever
            // gone: the row returns wearing the name the shelf showed, the
            // placement below lifts the removal, and the run reveals the
            // book at the end.
            let stone = ledger::find_tombstone(&folder, &file.fp).cloned();
            let store_at = match &origin {
                Origin::Stored { store, .. } => Some(store.clone()),
                Origin::Linked { .. } => None,
            };
            let mut book = Book::new(
                book_id,
                file.fp,
                file.format().unwrap_or(Format::Pdf),
                origin,
                now,
            );
            if let Some(title) = stone.as_ref().and_then(|s| s.title.clone()) {
                book.title = Some(title);
            }
            // A switch's copy is a book of its own beside the linked book the
            // old tree keeps: independent, so its marks and its place in it
            // are its own, and known by its copy's measurement — or by the
            // pending flag the startup sweep finishes, when the copy could
            // not be weighed. `add_book`'s one-row-per-fingerprint rule is
            // the right rule for a walk and the wrong one for a second
            // instance the reader just asked for by name, so the copy is
            // pushed past it.
            let placed_id = if switch_copy {
                book.independent = true;
                book.adopt_measurement(
                    store_at
                        .as_ref()
                        .and_then(|store| switch_measured.get(store))
                        .copied(),
                );
                let id = book.id.clone();
                books.push(Row::Book(book));
                id
            } else {
                add_book(books, book)
            };
            if stone.is_some() {
                restored.push(placed_id.clone());
            }
            let key = folder.shelf_key(file);
            let shelf_id = chain_for(
                &mut folder,
                &key,
                now,
                &root,
                &planned_name,
                merged,
                &mut new_shelves,
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
        // The planned tree's other half: the folder's books the library
        // already held, as memberships of the rows it holds them in. The
        // chain mints whatever rungs are not in the map yet — the whole tree
        // of an *as new* run, nothing at all of a merge into shelves that
        // stand — and the member guard below keeps a book that is already
        // where it is being put from moving to the end of it.
        for (row_id, file) in &replacements {
            let key = folder.shelf_key(file);
            let shelf_id = chain_for(
                &mut folder,
                &key,
                now,
                &root,
                &planned_name,
                merged,
                &mut new_shelves,
            );
            placements.push((row_id.clone(), shelf_id));
        }
    });

    state.library.shelves.update(|shelves| {
        for shelf in new_shelves {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        for (book_id, shelf_id) in &placements {
            let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) else {
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

    // The switch's own work, after the walk's: every book the tree read in
    // place becomes the library's copy on the shelf it already stands on.
    let converted = if switch_converts {
        convert_folder_books_to_stored(state, &task, &convert_ids).await
    } else {
        0
    };

    // A book that was removed and has just come back is revealed: the
    // import succeeded by making it reappear where the folder holds it, and
    // the highlight is how the reader is told so without a sentence. A
    // REPRESENTED file reveals the row its folder's log names instead: the
    // copy that came home is where the import "landed".
    let represented_count = represented.len() as u32;
    // One light for whichever came back first, restorations ahead of the rows a
    // log named as represented — the same order `run_files` reveals in, and the
    // same shape: a chain and a `next`, rather than two arms doing one thing.
    let came_back = restored.into_iter().chain(represented).next();
    if let Some(first) = came_back {
        super::reveal::reveal_book(state, &first);
    }

    // The asks are raised after the clean half landed and the blob was
    // written: the sheet counts against the level as the landing left it, and
    // a card that finishes with questions outstanding says so rather than
    // claiming an import nobody has answered yet.
    let waiting = asks.len() as u32;
    if !asks.is_empty() {
        conflict::raise(state, asks);
    }

    let total =
        placed + (relink_count + healed + replaced) as u32 + represented_count + converted as u32;
    finish_task(state, &task, total, waiting);
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
        folder_ops::find(folders, &folder_id).and_then(|f| {
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

        let now = now_ms();
        let book_id = id::next_id(now);
        let origin = if opts.in_place {
            Origin::Linked {
                src: found.path.clone(),
            }
        } else {
            match wire::copy_one_to_store(&task, &found.path, &book_id).await {
                Ok(store) => Origin::Stored {
                    src: Some(found.path.clone()),
                    store,
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
            let Some(folder) = folder_ops::find_mut(folders, &folder_id) else {
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
            if let Some(shelf) = shelves_ops::find_mut(shelves, &shelf_id) {
                shelves_ops::shelf_add(shelf, &placed_id);
            }
        });
        crate::storage::persist_library(state.library);
        // A removed book took its cover with it; a restored one gets it back
        // without asking to be opened first.
        super::covers::backfill_missing(state);
        finish_task(state, &task, 1, 0);
    });
}

/// The shelf a folder's root files onto, if it has one.
fn root_shelf_of(shelves: &[Shelf], folder_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| s.kind.is_folder_root() && s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}

/// Split one batch of copy results into the addresses that landed, and say
/// something about the ones that did not.
///
/// One spelling for both batches the library copies — an import's and a mode
/// switch's — because a per-file failure is the same news either way and the
/// reader should hear it in the same words. `noun` is the only thing that
/// differs and it is what the sentence counts: files on the way in, books on the
/// way over to the library's own copies.
///
/// A per-file failure is collected rather than fatal, which is the rule both
/// callers were already keeping: a folder with one locked file in it should
/// still import the other ninety-nine.
fn partition_store_results(
    state: AppState,
    results: Vec<StoreResult>,
    noun: &str,
) -> HashMap<String, String> {
    let mut landed = HashMap::new();
    let mut failures = Vec::new();
    for result in results {
        if result.is_ok() {
            landed.insert(result.id, result.store);
        } else {
            failures.push(file_name(&result.src));
        }
    }
    if !failures.is_empty() {
        let message = match failures.len() {
            1 => format!("Could not copy {}", failures[0]),
            n => format!("Could not copy {n} {noun}, starting with {}", failures[0]),
        };
        state.ui.toast.set(Some(Toast::new(message)));
    }
    landed
}

/// Copy one batch into the store, answering with the stored address per book id.
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
    Ok(partition_store_results(state, results, "files"))
}

/// The living linked rows whose fingerprint a folder's `placed` set holds —
/// the books its tree reads in place. The predicate both halves of the mode
/// switch run on: what a merge converts into copies, and what a replace puts
/// through the removal's sweep first.
fn linked_rows_of_placed(rows: &[Row], placed: &HashSet<Fingerprint>) -> Vec<String> {
    book_rows(rows)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && placed.contains(&b.fp))
        .map(|b| b.id.clone())
        .collect()
}

/// The rows a mode switch's *replace* would take out: the read-at-place
/// folder's own linked books. A stored book on one of its shelves — a copy
/// that came home — is NOT among them: the replace is about the instances
/// that read the OS folder, and a copy the library already owns is exactly
/// what the shelf ends up holding.
pub fn mode_switch_replace_rows(state: AppState, root: &str) -> Vec<String> {
    let placed: HashSet<Fingerprint> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .find(|f| f.root == root && f.opts.in_place)
            .map(|f| f.placed.clone())
            .unwrap_or_default()
    });
    if placed.is_empty() {
        return Vec::new();
    }
    state
        .library
        .books
        .with_untracked(|rows| linked_rows_of_placed(rows, &placed))
}

/// The replace answer's first half: the folder's linked books leave the
/// library through the removal's own sweep — row, memberships, cover,
/// highlights, and a tombstone per book in the folder's ledger. The copy
/// import that follows spends those logs as it lands, so the shelf comes
/// back holding only the library's copies, in the names the shelves showed.
pub(crate) fn purge_folder_linked_books(state: AppState, root: &str) {
    let doomed = mode_switch_replace_rows(state, root);
    if !doomed.is_empty() {
        super::arrange::purge_books(state, &doomed, PurgeOpts::default());
    }
}

/// The mode switch's *replace*, whole: the linked books go through the
/// sweep, and the folder walks again as the copies the reader asked for.
///
/// The claim is asked FIRST, and the check and the claim run in one
/// synchronous step (the webview is single-threaded, and nothing awaits
/// between them): a walk already in flight — a focus rescan of this very
/// folder is the realistic one — refuses the run with the sentence the
/// double-import always gets, and the purge simply does not happen. A
/// removal no import re-lands is the one outcome this ordering exists to
/// prevent.
pub(crate) fn replace_folder_with_copies(state: AppState, root: String, opts: FolderOpts) {
    if root_is_claimed(&root) {
        already_importing(state, &root);
        return;
    }
    purge_folder_linked_books(state, &root);
    proceed_folder(state, root, opts, RootPlan::default());
}

/// Measure a batch of store copies in one pass: stored address to its
/// fingerprint. A copy that cannot be measured is simply absent, and the row
/// it belongs to keeps a pending flag the startup sweep finishes.
async fn measure_stores(stores: Vec<String>) -> HashMap<String, Fingerprint> {
    if stores.is_empty() {
        return HashMap::new();
    }
    wire::verify_paths(stores)
        .await
        .ok()
        .map(|checks| {
            checks
                .into_iter()
                .filter_map(|check| Some((check.path.clone(), check.fingerprint()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// The mode switch's merge, second half: every book the tree read in place
/// becomes the library's own copy WHERE IT STANDS — the same row, so its id,
/// its name, its shelves, its resume point and its highlights all survive the
/// flip, and only the bytes' home and the row's identity change.
///
/// The copy takes its own measurement as the row's fingerprint, and the
/// ORIGINAL stays in the folder's `placed` set with no row wearing it — the
/// departure rule's arithmetic once more, and what keeps a later rescan
/// quiet about a file whose book now lives in the store: the registry
/// answers nothing for the original and the ledger answers "this folder
/// placed it", which is a skip rather than a second book.
///
/// A per-file failure is collected rather than fatal: a book that could not
/// be copied keeps reading in place, the folder is stable with copies for
/// some of its books and links for the rest, and a re-import offers the
/// switch again for the ones that remain.
async fn convert_folder_books_to_stored(state: AppState, task: &str, ids: &[String]) -> usize {
    // Live facts per row — the address and the highlight key BEFORE the flip
    // — because the walk this runs behind may have relinked or healed a row
    // the switch started from, and a row that is no longer linked (a
    // departure beat the switch to it) is none of this run's business.
    let candidates: Vec<(String, String, String)> = state.library.books.with_untracked(|rows| {
        ids.iter()
            .filter_map(|id| {
                let book = find_row(rows, id)?.book()?;
                matches!(book.origin, Origin::Linked { .. })
                    .then(|| (id.clone(), book.path().to_string(), book.gloss_key()))
            })
            .collect()
    });
    if candidates.is_empty() {
        return 0;
    }
    let requests: Vec<StoreRequest> = candidates
        .iter()
        .map(|(id, path, _)| StoreRequest {
            path: path.clone(),
            id: id.clone(),
        })
        .collect();
    let results = match wire::store_books(task, &requests).await {
        Ok(results) => results,
        Err(message) => {
            state.ui.toast.set(Some(Toast::new(message)));
            return 0;
        }
    };
    let stores = partition_store_results(state, results, "books");
    let measured = measure_stores(stores.values().cloned().collect()).await;
    let mut converted = 0usize;
    for (id, path, from_key) in candidates {
        let Some(store) = stores.get(&id) else {
            continue;
        };
        let fp = measured.get(store).copied();
        state.library.books.update(|rows| {
            let Some(book) = find_book_mut(rows, &id) else {
                return;
            };
            // The same write a single departure makes: a switch is a whole
            // shelf of them, and the two have to agree about what travels.
            book.become_stored(&path, store.clone(), fp);
        });
        // The highlights follow the address, by the departure's own rule:
        // moved outright when no remaining row reads the old one, copied
        // when a twin still does. The marks keep their ids, so the AI
        // answers ride along with them.
        let to_key = state.library.books.with_untracked(|rows| {
            find_row(rows, &id)
                .and_then(|row| row.book())
                .map(Book::gloss_key)
        });
        if let Some(to) = to_key
            && to != from_key
        {
            migrate_gloss(state, &from_key, &to, &path);
        }
        converted += 1;
    }
    if converted > 0 {
        // The old addresses' covers belong to files no row reads any more,
        // and the copies have never been rendered: prune one side, queue the
        // other, and write the blob once for the whole shelf.
        super::covers::prune_now(state);
        super::covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
    }
    converted
}

/// Import loose files: measure them, then answer each one by the ground it
/// stands on — a file of a read-at-place folder is that folder's business
/// first, and the rest land as the library's own stored copies, except the
/// ones whose NAME the target level already holds, which ask (see
/// [`crate::services::library::conflict`]).
///
/// A loose file has no folder to rescan it and no structure to preserve, and
/// a linked row no ledger answers for is a row no rule can keep honest — so
/// the import is a copy: the bytes go into the store, the row's identity is
/// the copy's own measurement, and the source file's fingerprint stays free
/// for any folder that reads it. The one file this does NOT copy silently is
/// a file an in-place folder's tree already holds: that folder already
/// answers for it, so the drop becomes the folder's own two-answer question
/// — the library's copy here, or the folder's book lit where it stands — and
/// a file the folder's log remembers removing or moving out comes BACK in its
/// folder's place instead, the log spent by the explicit ask.
async fn run_files(state: AppState, task: String, paths: Vec<String>, target: Option<String>) {
    let checks = match wire::verify_paths(paths).await {
        Ok(checks) => checks,
        Err(message) => return fail(state, &task, message, false),
    };
    let mut found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
    // A file some folder's log says is REPRESENTED by a returned stored copy
    // succeeds the drop by lighting that row up rather than landing a linked
    // neighbour beside it — the folder rule, on the loose-file side.
    let mut represented: Vec<String> = Vec::new();
    found.retain(|file| {
        let Some(row_id) = state.library.folders.with_untracked(|folders| {
            folders.iter().find_map(|folder| {
                ledger::find_tombstone(folder, &file.fp)
                    .and_then(|entry| entry.returned_row.clone())
            })
        }) else {
            return true;
        };
        let alive = state
            .library
            .books
            .with_untracked(|rows| find_row(rows, &row_id).is_some());
        if alive {
            represented.push(row_id);
            false
        } else {
            true
        }
    });
    if found.is_empty() && represented.is_empty() {
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

    let shelf_id = target
        .clone()
        .unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());

    // The read-at-place folder's answer, before the level's name question: a
    // file an in-place tree holds is a file the library already has a book
    // for, and what the drop means is the folder's to say — a logged book
    // comes back where the folder holds it, a standing book asks the covered
    // question, and only a file no tree answers for is an ordinary import.
    let mut restored: Vec<String> = Vec::new();
    let mut covered_asks: Vec<ConflictAsk> = Vec::new();
    found.retain(|file| match covered_fate(state, file) {
        CoveredFate::Ordinary => true,
        CoveredFate::Restore { folder_id, stone } => {
            restored.push(restore_covered_file(state, file, &folder_id, &stone));
            false
        }
        CoveredFate::Ask { folder_id, row_id } => {
            // Named before the id is moved: the row's own name is what the
            // sheet prints, and reading it after the constructor took the id
            // would be reading a value that is no longer here.
            let existing_name = state.library.row_name(&row_id);
            covered_asks.push(ConflictAsk::covered(
                Arrival::import(file.clone(), shelf_id.clone(), None),
                row_id,
                existing_name,
                folder_id,
            ));
            false
        }
    });
    // The books that came back are on their shelf already; write them before
    // the copies run, so a failure below cannot lose a restoration.
    if !restored.is_empty() {
        crate::storage::persist_library(state.library);
    }

    // Every file whose NAME the target level already holds is a question
    // rather than a placement — the old rule resolved the file to the row the
    // library already had and skipped the placement, which to the reader was a
    // book swallowed by the shelf it was dropped on. The question is the level's
    // and is about a name, so a file whose twin sits on another shelf is not
    // one: the row the library already has simply gains this level as well,
    // which is a landing the reader can see and the one they asked for by
    // dropping here.
    let arrivals: Vec<Arrival> = found
        .iter()
        .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
        .collect();
    let (clean, conflicts) = conflict::screen(state, arrivals);

    // The clean half lands as the library's own copies: one batch for the
    // whole drop rather than one copy per file, and one measurement pass over
    // the copies that landed — a drop of four hundred files is one walk of
    // the store and one write of the blob.
    let now = now_ms();
    let mut pending: Vec<(String, FoundFile, Option<String>, Option<usize>)> = Vec::new();
    let mut stone_landings: Vec<String> = Vec::new();
    for arrival in &clean {
        let Some(file) = arrival.file.clone() else {
            continue;
        };
        // A file some folder remembers removing comes back wearing the name
        // its shelf showed, wherever it lands, and the removal is spent by
        // the explicit ask. An IN-PLACE tree's log never reaches here — the
        // folder's answer above owns those files — so what this lifts is a
        // copying folder's log, or one whose file has since left the tree.
        let stone = lift_stone_for(state, &file);
        let title = stone.as_ref().and_then(|s| s.title.clone());
        let book_id = id::next_id(now);
        if stone.is_some() {
            stone_landings.push(book_id.clone());
        }
        pending.push((book_id, file, title, arrival.index));
    }
    let requests: Vec<(String, &FoundFile)> = pending
        .iter()
        .map(|(book_id, file, _, _)| (book_id.clone(), file))
        .collect();
    let copies = if requests.is_empty() {
        HashMap::new()
    } else {
        match copy_batch(state, &task, &requests).await {
            Ok(copies) => copies,
            Err(message) => {
                // The questions survive the failure: they are about the
                // library rather than about the store, and every answer can
                // still land — or fail — on its own terms.
                let mut asks = covered_asks;
                asks.extend(conflicts);
                conflict::raise(state, asks);
                return fail(state, &task, message, false);
            }
        }
    };
    // The copies' own measurements, in one pass: a stored row's identity is
    // the copy's fingerprint and the source file's stays free — the departure
    // rule's arithmetic on the import's side. A copy that cannot be measured
    // leaves the pending flag, which the startup sweep finishes.
    let stores: Vec<String> = pending
        .iter()
        .filter_map(|(book_id, _, _, _)| copies.get(book_id).cloned())
        .collect();
    let measured = measure_stores(stores).await;
    let mut landed = 0u32;
    for (book_id, file, title, index) in pending {
        // A per-file failure was the batch's own toast; a copy that did not
        // land leaves no row behind.
        let Some(store) = copies.get(&book_id) else {
            continue;
        };
        mint_stored_row(
            state,
            book_id.clone(),
            &file,
            store.clone(),
            title,
            &shelf_id,
            index,
        );
        adopt_copy_measurement(state, &book_id, measured.get(store).copied());
        landed += 1;
    }
    let stone_landings: Vec<String> = stone_landings
        .into_iter()
        .filter(|book_id| copies.contains_key(book_id))
        .collect();
    // A represented file is a succeeded import as much as a landed one: the
    // card says what the drop was worth, and the highlight below says where.
    let placed = landed
        + (restored.len() + stone_landings.len()) as u32
        + represented.len() as u32;
    let waiting = (conflicts.len() + covered_asks.len()) as u32;
    // The landed rows may read from addresses the cover cache has no art for:
    // one ask for the batch rather than one per file.
    if placed > 0 {
        super::covers::backfill_missing(state);
    }
    // One light for the books that CAME BACK — the folder's restorations
    // first, then the landings a log was spent by, then the rows the logs
    // named as represented: "it is back" is worth a highlight and no
    // sentence.
    let came_back = restored
        .into_iter()
        .chain(stone_landings)
        .chain(represented)
        .next();
    if let Some(first) = came_back {
        super::reveal::reveal_book(state, &first);
    }
    // Raised after the clean half landed: the sheet counts the questions, and
    // a landing that shifted a member list is one the answers resolve against.
    // The covered questions go first: the folder's answer was asked first, in
    // the walk's own order.
    let mut asks = covered_asks;
    asks.extend(conflicts);
    conflict::raise(state, asks);
    crate::storage::persist_library(state.library);
    finish_task(state, &task, placed, waiting);
}

/// What the read-at-place folder this file stands in already says about a
/// loose import of it.
#[derive(Debug)]
enum CoveredFate {
    /// No in-place folder's tree holds this address: the file is an ordinary
    /// import, and lands as the library's own copy through the level's name
    /// question. A tree that covers the ground but never placed THIS file —
    /// new since the last scan, or outside the folder's filters — answers
    /// here too: the import is the library's copy, and the folder places its
    /// own linked book on the walk that finds it, as it always would.
    Ordinary,
    /// A folder that holds the file has a log for it — a removal, or a
    /// moved-out log whose copy has since died — and an explicit import
    /// spends the log the way a folder walk does: the book comes back as the
    /// folder's own linked book, in its folder's place, wearing the name the
    /// shelf showed, and the run lights it up.
    Restore { folder_id: String, stone: Tombstone },
    /// The folder's book for this file is alive and standing: the import is
    /// the covered question — the library's own stored copy on this level,
    /// or the folder's book lit where it stands — because a second linked
    /// row of one read-at-place file is the one thing the folder rule never
    /// makes.
    Ask { folder_id: String, row_id: String },
}

/// Ask the in-place folders what they hold for one loose file.
///
/// A living row at the file's very address answers FIRST, and the order is
/// the walk's own rather than a preference: an explicit folder run reads the
/// registry before it reads the logs (`decide_import`), and a log standing
/// beside a living row of its fingerprint is a stale state the next walk's
/// `prune_tombstones` drops — a hand-open between the removal and the import
/// is how it happens. Answering the row writes nothing, so it cannot
/// duplicate the book that is there; answering the log would mint a second
/// linked row over it, which is the one thing the covered question exists to
/// prevent. The row IS the folder's book — a linked row stays in its
/// folder's tree, because every departure converts it — and an import of the
/// file is the question, never a second linked instance beside it.
///
/// The LOG answers second: a removal, or a moved-out log whose copy has
/// since died, is spent by the explicit import, and the book comes back in
/// its folder's place, exactly as a walk re-importing the folder brings it.
/// (A moved-out log BOUND to a living copy was spent by the `represented`
/// check before this runs, so it never reaches here.)
fn covered_fate(state: AppState, file: &FoundFile) -> CoveredFate {
    // The in-place folders whose tree holds the file's address.
    let covering: Vec<String> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|f| f.opts.in_place && rel_under(&file.path, &f.root).is_some())
            .map(|f| f.id.clone())
            .collect()
    });
    if covering.is_empty() {
        return CoveredFate::Ordinary;
    }
    let row_id = state.library.books.with_untracked(|rows| {
        book_rows(rows)
            .find(|b| b.path() == file.path)
            .map(|b| b.id.clone())
    });
    if let Some(row_id) = row_id {
        // The folder the sheet names is the one that PLACED the file; a tree
        // whose ledger lost the fingerprint — the file changed since the walk
        // that placed it — falls back to the first that covers the address.
        let folder_id = state
            .library
            .folders
            .with_untracked(|folders| {
                covering
                    .iter()
                    .find(|id| {
                        folders
                            .iter()
                            .any(|f| &f.id == *id && f.placed.contains(&file.fp))
                    })
                    .cloned()
            })
            .unwrap_or_else(|| covering[0].clone());
        return CoveredFate::Ask { folder_id, row_id };
    }
    let stoned = state.library.folders.with_untracked(|folders| {
        covering.iter().find_map(|id| {
            folder_ops::find(folders, id)
                .and_then(|f| ledger::find_tombstone(f, &file.fp).cloned())
                .map(|stone| (id.clone(), stone))
        })
    });
    match stoned {
        Some((folder_id, stone)) => CoveredFate::Restore { folder_id, stone },
        None => CoveredFate::Ordinary,
    }
}

/// The write half of [`CoveredFate::Restore`]: the folder's book comes back
/// the way a folder walk brings it back — a LINKED book at the file's
/// address, wearing the name the shelf showed, on the folder's own ground —
/// and the log is spent by the landing. Returns the row so the run can light
/// it up.
///
/// The shelf is the one the log remembers when it still stands, then the
/// folder's mapped rung for the file's subfolder, then the folder's root
/// shelf: a book that came back should not come back somewhere new, and
/// least of all on the level the file happened to be dropped on — the drop
/// asked for a file the folder owns, and the folder's place is the answer.
/// A folder with no shelf left at all leaves the book in the library unfiled,
/// which is the restore menu's own fallback.
fn restore_covered_file(
    state: AppState,
    file: &FoundFile,
    folder_id: &str,
    stone: &Tombstone,
) -> String {
    // The folder's two rungs for this file: the one its subfolder maps to,
    // and the one at its root.
    let (rung, root_rung) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, folder_id)
            .map(|f| {
                let rel = rel_under(&file.path, &f.root).unwrap_or_default();
                let key = match rel.rsplit_once('/') {
                    Some((dir, _)) if f.opts.groups => dir,
                    _ => "",
                };
                (f.shelf_map.get(key).cloned(), f.shelf_map.get("").cloned())
            })
            .unwrap_or_default()
    });
    let target = state.library.shelves.with_untracked(|shelves| {
        let standing = |id: &Option<String>| {
            id.as_deref()
                .filter(|sid| shelves.iter().any(|s| s.id == **sid))
                .map(str::to_string)
        };
        standing(&stone.shelf_id)
            .or_else(|| standing(&rung))
            .or_else(|| standing(&root_rung))
            .or_else(|| root_shelf_of(shelves, folder_id))
    });
    // The log comes out and the placement is marked in one write: a
    // fingerprint the ledger skips with no book behind it is the one state a
    // folder cannot recover from on its own. (`placed` kept the fingerprint
    // through the removal and the departure alike; the mark is the guarantee.)
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            ledger::restore_deleted(folder, &file.fp);
            folder.mark_placed(file.fp);
        }
    });
    let shelf_id = target.unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());
    land_file(state, file, stone.title.clone(), &shelf_id, None)
}

/// The removal a folder holds for this file, lifted — `None` when no folder
/// remembers removing it.
///
/// The match is the FINGERPRINT, not the address: a file that was removed
/// from a folder, moved across the disk by the OS or by hand, and dropped
/// back into the library is the same file the log was written for, and an
/// explicit import of it is the reader asking for that book again. The log
/// that kept a watchful rescan from resurrecting the book is spent by the
/// ask; what comes back is the book, in its old name, with a highlight.
fn lift_stone_for(state: AppState, file: &FoundFile) -> Option<Tombstone> {
    let owner = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .find(|f| ledger::find_tombstone(f, &file.fp).is_some())
            .map(|f| f.id.clone())
    })?;
    let mut stone = None;
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, &owner) {
            stone = ledger::restore_deleted(folder, &file.fp);
        }
    });
    stone
}

/// Land one measured file as a book row on one level.
///
/// The landing of a file a FOLDER answers for: an in-place merge seating a
/// file its own rung holds, and a restoration putting a logged book back in
/// its folder's place. A loose import does not come through here — a file no
/// folder answers for is the library's own stored copy
/// ([`land_stored_copy`]), because a linked row no ledger keeps is a row no
/// rescan can heal, tombstone or hand back. `name` is the title the row is
/// given: `None` leaves it without one, so the stem of its address is the name
/// the shelf shows, which is the honest name for a file nobody has opened yet,
/// and `Some` is the name a log or a sheet gave it.
///
/// A second row of an address the library already reads is a book of its own
/// ([`Book::independent`]): its highlights live under a key carrying its id and
/// its resume point is written by itself, which is what makes "add as new"
/// mean something a reader can see rather than a second name for one book.
///
/// The row is always a new one, named or not. Either way the reader asked for
/// a book HERE, and a resolve to the row another level holds would answer
/// with nothing new on the level they dropped on — the vanishing the
/// collision sheet exists to stop.
///
/// Writes no persist and queues no cover, because a caller that lands four
/// hundred files owes ONE write and ONE cover ask — the queue re-derives its
/// whole want-list on every call — and only the caller knows whether this is
/// one file or four hundred.
pub fn land_file(
    state: AppState,
    file: &FoundFile,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    mint_row(
        state,
        file,
        Origin::Linked {
            src: file.path.clone(),
        },
        None,
        name,
        shelf_id,
        index,
    )
}

/// Land a file whose bytes the app copied into its store: the row
/// [`land_file`] lands, at a stored address, under an id minted BEFORE the
/// copy — the stored file is named after it, and a mint afterwards would
/// leave two copies of one name fighting over one slot in the store.
pub(crate) fn mint_stored_row(
    state: AppState,
    book_id: String,
    file: &FoundFile,
    store: String,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    mint_row(
        state,
        file,
        Origin::Stored {
            src: Some(file.path.clone()),
            store,
        },
        Some(book_id),
        name,
        shelf_id,
        index,
    )
}

/// Land one loose file as the library's own stored copy: the single-file form
/// of the batch [`run_files`] lands, for the two answers that place a file
/// after a question — the name sheet's *add as new* and the covered sheet's
/// *import here*.
///
/// The copy is made BEFORE the row is promised — a failure to copy is a toast
/// and a level left untouched, the one honest outcome for a file that could
/// not be filed — and the id is minted now because the stored file is named
/// after it. The row then takes the copy's own measurement as its identity
/// ([`adopt_copy_measurement`]), which is what leaves the source file's
/// fingerprint free for the folders that read it.
pub(crate) fn land_stored_copy(
    state: AppState,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
) {
    let book_id = id::next_id(now_ms());
    let task = format!("import-{book_id}");
    spawn_local(async move {
        match wire::copy_one_to_store(&task, &file.path, &book_id).await {
            Ok(store) => {
                let measured = wire::verify_paths(vec![store.clone()])
                    .await
                    .ok()
                    .and_then(|checks| checks.into_iter().next())
                    .and_then(|check| check.fingerprint());
                let placed =
                    mint_stored_row(state, book_id, &file, store, name, &shelf_id, index);
                adopt_copy_measurement(state, &placed, measured);
                super::covers::backfill_missing(state);
                crate::storage::persist_library(state.library);
            }
            Err(message) => state.ui.toast.set(Some(Toast::new(message))),
        }
    });
}

/// The copy's own measurement becomes the row's identity, and the source
/// file's fingerprint is left free — the departure rule's arithmetic on the
/// import's side. A stored book is the library's own instance, and an OS file
/// that keeps its own fingerprint can still be placed by any folder that
/// reads it, as its own linked book, instead of being answered away as
/// "content the library holds" by a registry that only saw the store's copy
/// of it. A copy that could not be measured leaves the pending flag rather
/// than blocking the landing; the startup sweep re-measures the store path
/// and finishes the job.
fn adopt_copy_measurement(state: AppState, row_id: &str, measured: Option<Fingerprint>) {
    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.adopt_measurement(measured);
        }
    });
}

/// The row both landings mint.
fn mint_row(
    state: AppState,
    file: &FoundFile,
    origin: Origin,
    book_id: Option<String>,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    let now = now_ms();
    let independent = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.path() == file.path));
    let book = Book {
        title: name,
        independent,
        ..Book::new(
            book_id.unwrap_or_else(|| id::next_id(now)),
            file.fp,
            file.format().unwrap_or(Format::Pdf),
            origin,
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
    // The root has no member list to place into, so the write is skipped rather
    // than made and answered with nothing: a batch landing four hundred files
    // "on All" would otherwise notify the shelf list four hundred times.
    if shelf_id != shelves_ops::ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, &placed, index);
            }
        });
    }
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
    use super::{
        CoveredFate, claim_root, covered_fate, covered_shelf, land_file,
        mode_switch_replace_rows, purge_folder_linked_books, rel_of, restore_covered_file,
        shelf_name,
    };
    use crate::state::AppState;
    use leptos::prelude::*;
    use library_core::book::{Book, Fingerprint, Origin, Row};
    use library_core::folder::{FolderOpts, Tombstone, WatchedFolder};
    use library_core::scan::FoundFile;
    use library_core::shelf::Shelf;
    use reader_core::format::Format;

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

    // -------------------------------------------------------------------
    // The read-at-place folder's answer to a loose file.
    // -------------------------------------------------------------------

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    /// An in-place folder that placed the given fingerprints, with the logs
    /// it holds — the ledger half of a read-at-place import.
    fn folder(id: &str, root: &str, placed: &[u32], ignored: Vec<Tombstone>) -> WatchedFolder {
        WatchedFolder {
            id: id.to_string(),
            root: root.to_string(),
            opts: FolderOpts::default(),
            placed: placed.iter().copied().map(fp).collect(),
            ignored,
            shelf_map: Default::default(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        }
    }

    /// A removal's log for `n`, filed on `shelf` when it was filed on one.
    fn stone(n: u32, path: &str, moved: bool, shelf: Option<&str>) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some("Dune".to_string()),
            format: Format::Markdown,
            last_path: path.to_string(),
            shelf_id: shelf.map(str::to_string),
            removed_ms: 5,
            moved,
            returned_row: None,
        }
    }

    #[test]
    fn a_file_an_in_place_folder_holds_is_a_question_not_a_second_link() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found("/books/dune.md", 7);
        state.library.shelves.set(vec![plain("fs"), plain("s")]);
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);
        // The folder's own book for the file, standing where the folder put
        // it — the row an import of the same file must never duplicate.
        let landed = land_file(state, &file, None, "fs", None);

        match covered_fate(state, &file) {
            CoveredFate::Ask { folder_id, row_id } => {
                assert_eq!(folder_id, "f1", "the folder that placed the file is the one asked about");
                assert_eq!(row_id, landed, "and the question names the book it holds");
            }
            other => panic!("expected the folder's question, got {other:?}"),
        }

        // A COPYING folder's tree is no cover: its books are the library's
        // own copies, and the OS file stays an ordinary import.
        let mut copying = folder("f2", "/books", &[7], Vec::new());
        copying.opts.in_place = false;
        state.library.folders.set(vec![copying]);
        assert!(
            matches!(covered_fate(state, &file), CoveredFate::Ordinary),
            "a copying folder's tree asks nothing"
        );
    }

    #[test]
    fn a_file_a_folder_log_remembers_comes_back_to_the_folders_place() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found("/books/scifi/dune.md", 7);
        state.library.shelves.set(vec![plain("fs"), plain("sub"), plain("s")]);
        let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/scifi/dune.md", false, Some("fs"))]);
        one.shelf_map.insert(String::new(), "fs".to_string());
        one.shelf_map.insert("scifi".to_string(), "sub".to_string());
        state.library.folders.set(vec![one]);

        let fate = covered_fate(state, &file);
        assert!(
            matches!(fate, CoveredFate::Restore { .. }),
            "the log answers before any question: got {fate:?}"
        );
        let CoveredFate::Restore { folder_id, stone } = fate else {
            unreachable!()
        };

        let id = restore_covered_file(state, &file, &folder_id, &stone);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1, "the folder's book is back, and it is the only book");
        let book = rows[0].book().expect("a book row");
        assert_eq!(book.id, id);
        assert_eq!(book.path(), "/books/scifi/dune.md", "linked — it is the folder's file again");
        assert!(matches!(book.origin, Origin::Linked { .. }));
        assert_eq!(book.title.as_deref(), Some("Dune"), "wearing the name the shelf showed");
        let shelves = state.library.shelves.get_untracked();
        let on = |sid: &str| {
            shelves
                .iter()
                .find(|s| s.id == sid)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            on("fs"),
            vec![id],
            "on the shelf the log remembers — not the one the file was dropped on"
        );
        assert!(on("sub").is_empty() && on("s").is_empty());
        let folders = state.library.folders.get_untracked();
        assert!(folders[0].ignored.is_empty(), "the log is spent by the landing");
        assert!(
            folders[0].placed.contains(&file.fp),
            "and the folder still answers for the file, so no rescan doubles it"
        );
    }

    #[test]
    fn a_moved_out_log_with_no_copy_behind_it_brings_the_linked_book_back() {
        // The move made a copy and the copy has since died (a merge folded
        // it away): the log is unbound, and an import of the OS file is owed
        // a real linked book in the folder's place rather than a highlight.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found("/books/dune.md", 7);
        state.library.shelves.set(vec![plain("fs")]);
        let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/dune.md", true, Some("fs"))]);
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);

        match covered_fate(state, &file) {
            CoveredFate::Restore { folder_id, stone } => {
                assert_eq!(folder_id, "f1");
                assert!(stone.moved, "the log it spends is the moved-out one");
            }
            other => panic!("expected the folder's book to come back, got {other:?}"),
        }
    }

    #[test]
    fn a_living_row_outvotes_a_stale_log_beside_it() {
        // The state the next walk prunes — a log standing while a row reads
        // the address, which a hand-open between the removal and the import
        // is how happens — gets the walk's own answer: the registry speaks
        // before the logs, so the import asks about the book that IS there
        // rather than minting a second linked row over it.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found("/books/dune.md", 7);
        state.library.shelves.set(vec![plain("fs")]);
        state.library.books.set(vec![linked("b1", "/books/dune.md", 7)]);
        let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/dune.md", false, Some("fs"))]);
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);

        assert!(
            matches!(covered_fate(state, &file), CoveredFate::Ask { row_id, .. } if row_id == "b1"),
            "the row that is there is the book the import asks about"
        );
    }

    #[test]
    fn a_file_no_in_place_tree_answers_for_is_an_ordinary_import() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.shelves.set(vec![plain("fs")]);
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);

        // Under the tree, but a file the folder never placed — new since the
        // last walk, or outside its filters. No book to show and no log to
        // spend: an ordinary import, and the folder places its own linked
        // book on the walk that finds it.
        let fresh = found("/books/new.md", 9);
        assert!(matches!(covered_fate(state, &fresh), CoveredFate::Ordinary));
        // And outside every tree altogether.
        let outside = found("/elsewhere/notes.md", 8);
        assert!(matches!(covered_fate(state, &outside), CoveredFate::Ordinary));
        // A subdirectory spelling of the same fact: "/books2" is not inside
        // "/books", however much the prefix looks like it.
        let neighbour = found("/books2/dune.md", 7);
        assert!(matches!(covered_fate(state, &neighbour), CoveredFate::Ordinary));
    }

    // -------------------------------------------------------------------
    // The read-at-place gate, and the mode switch's sharp edge.
    // -------------------------------------------------------------------

    fn linked(id: &str, path: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked {
                src: path.to_string(),
            },
            0,
        ))
    }

    fn stored(id: &str, src: &str, store: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Stored {
                src: Some(src.to_string()),
                store: store.to_string(),
            },
            0,
        ))
    }

    #[test]
    fn the_gate_answers_by_the_rung_the_pick_names() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        one.shelf_map.insert("scifi".to_string(), "sub".to_string());
        state.library.folders.set(vec![one]);
        state.library.shelves.set(vec![plain("fs"), plain("sub")]);

        // The tree's own root: the empty rung, which is the continuation's
        // shape — a walk, and the note only if the walk finds nothing.
        let (rel, id, name) = covered_shelf(state, "/books").expect("covered");
        assert_eq!(rel, "");
        assert_eq!(id, "fs");
        assert_eq!(name, "fs");

        // A rung inside the tree: the note's shape, lit on the rung itself.
        let (rel, id, _) = covered_shelf(state, "/books/scifi").expect("covered");
        assert_eq!(rel, "scifi");
        assert_eq!(id, "sub");

        // Ground no in-place tree holds is no gate at all.
        assert!(covered_shelf(state, "/other").is_none());
        // A copying folder's tree is not the gate's business: its copies are
        // the library's to make another of.
        let mut copying = folder("f2", "/comics", &[], Vec::new());
        copying.opts.in_place = false;
        copying.shelf_map.insert(String::new(), "cs".to_string());
        state.library.folders.update(|folders| folders.push(copying));
        state.library.shelves.update(|shelves| shelves.push(plain("cs")));
        assert!(covered_shelf(state, "/comics").is_none());
        // A rung whose shelf has died is no standing shelf: the walk may
        // mint it again, so the gate stands aside.
        state
            .library
            .shelves
            .update(|shelves| shelves.retain(|each| each.id != "sub"));
        assert!(covered_shelf(state, "/books/scifi").is_none());
    }

    #[test]
    fn a_folder_own_tree_answers_for_it_before_a_tree_it_stands_inside() {
        // Two in-place trees, one inside the other: the inner folder's own
        // root shelf is the gate's answer for its root, not the outer tree's
        // rung for it — whichever order the folders are stored in.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut inner = folder("f1", "/books/scifi", &[7], Vec::new());
        inner.shelf_map.insert(String::new(), "sub".to_string());
        let mut outer = folder("f2", "/books", &[8], Vec::new());
        outer.shelf_map.insert(String::new(), "fs".to_string());
        outer.shelf_map.insert("scifi".to_string(), "outersub".to_string());
        state.library.folders.set(vec![outer, inner]);
        state
            .library
            .shelves
            .set(vec![plain("fs"), plain("outersub"), plain("sub")]);

        let (rel, id, _) = covered_shelf(state, "/books/scifi").expect("covered");
        assert_eq!(rel, "", "the folder's own tree is the empty rung");
        assert_eq!(id, "sub", "and its own root shelf is the light");
    }

    #[test]
    fn a_replace_takes_the_linked_books_and_leaves_the_copies() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        // The folder's two linked books, and a stored copy that came home
        // onto one of its shelves: the replace is about the instances that
        // read the OS folder, and the copy is exactly what the shelf ends up
        // holding, so it stands.
        state.library.books.set(vec![
            linked("b1", "/books/a.md", 1),
            linked("b2", "/books/b.md", 2),
            stored("b3", "/books/c.md", "/store/b3.md", 3),
        ]);
        let mut shelf = plain("fs");
        shelf.books = vec!["b1".to_string(), "b2".to_string(), "b3".to_string()];
        state.library.shelves.set(vec![shelf]);
        let mut one = folder("f1", "/books", &[1, 2], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);

        let mut doomed = mode_switch_replace_rows(state, "/books");
        doomed.sort();
        assert_eq!(
            doomed,
            vec!["b1".to_string(), "b2".to_string()],
            "the linked books the folder's ledger answers for, and nothing else"
        );

        purge_folder_linked_books(state, "/books");

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 1, "the copy that came home is what stands");
        assert_eq!(rows[0].id(), "b3");
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves[0].books,
            vec!["b3".to_string()],
            "the linked books came off the shelf they were filed on"
        );
        let folders = state.library.folders.get_untracked();
        assert_eq!(
            folders[0].ignored.len(),
            2,
            "and the folder's ledger remembers them — the logs the copy import spends as it lands"
        );
        assert!(folders[0].ignored.iter().all(|entry| !entry.moved));
        assert!(
            folders[0].placed.contains(&fp(1)) && folders[0].placed.contains(&fp(2)),
            "the placements stay: they are what keeps a rescan quiet until the copies land"
        );
    }
}
