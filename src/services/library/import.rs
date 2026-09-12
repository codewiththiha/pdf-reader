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
//!   * **the tree on disk is the tree on the shelf — and a hand cannot take a
//!     read-at-place rung off it.** A folder import mints the whole chain of
//!     shelves between the watched root and each file's subfolder, and a rescan
//!     re-hangs the folder's shelves on the rung their `rel` names, so importing
//!     "1" that holds "2", "3" and four books yields "1" at the root with "2",
//!     "3" and the books inside it — one logic, one tree, rather than a flat
//!     shelf list grown beside a nested one. A hand-move of a rung a READ-AT-
//!     PLACE folder named is a departure rather than a re-hang to fight: the
//!     shelf leaves as a copy the library owns (`crate::services::library::arrange`
//!     asks, copies and converts it) and the folder's map lets the zone go, so
//!     the next walk mints the original rung back on the seat the disk names.
//!     The re-hang still passes by a shelf wearing `Shelf::manual_parent` — a
//!     merged import's rungs and a COPYING folder's shelves, which a hand may
//!     take and keep. Virtual shelves are the reader's own and no scan ever
//!     rearranges them.
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
    find_row,
};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops, FolderOpts, Tombstone, WatchedFolder, rel_under};
use library_core::id;
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};
use library_core::wire::{PathCheck, StoreRequest, StoreResult};
use reader_core::format::{Format, is_supported_path};

use super::arrange::PurgeOpts;
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
///   * `fold` — the family tree and the rung key this run's folder goes back
///     to when the walk ends: `(tree id, rel)`. An import of a folder is the
///     reader wanting it BACK, and back is the rung its directory names in the
///     in-place tree that covers it — so the walk runs on the pick's own
///     ledger and the run's last act folds the standing shelf into the family
///     (`reclaim_rung`), the seat the disk names rather than the level the
///     pick would otherwise mint on. Set by the gate for a pick inside a
///     family whose rung is gone, and for a re-pick of a folder that is
///     itself standing outside its family; a rescan never carries one.
#[derive(Clone, Default)]
pub(crate) struct RootPlan {
    pub rename: Option<String>,
    pub into: Option<String>,
    pub continuation: Option<(String, String)>,
    pub fold: Option<(String, String)>,
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
/// One spelling, because the two doors that can refuse a run — an import and a
/// replace — refuse it for the same reason and owe the reader the same
/// words. A toast each door worded itself would eventually differ about
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
/// Import a folder, with the options the sheet was filled in with. Returns
/// immediately: the dock owns the feedback from here on.
///
/// A folder whose NAME the root level already holds is a question before it
/// is an import — two shelves of one name on one level are two doors a reader
/// cannot tell apart, which is the shelf's own spelling of the collision the
/// book sheet asks about. The question goes to the folder sheet
/// (`crate::services::library::conflict`), and the run starts with the answer
/// it gave as its [`RootPlan`]. ALWAYS: a reader who picked a folder and
/// clicked Import asked for an answer, and a run that ends on "Imported 0
/// books" with no sheet in between is the silent nothing the book collision
/// used to be. A folder colliding with its OWN previous shelf asks too — the
/// continuation is a choice rather than a surprise.
///
/// The read-at-place gate in front of all of it belongs to the READ-AT-PLACE
/// arrival alone, and it is the family's answer rather than the level's
/// (`covered_shelf`, `library_core::shelf::family_for`). A linked shelf IS an
/// OS folder, so which tree the pick belongs to is a question about the
/// ground and not about where the reader happens to be standing:
///
///   * a RUNG the family already holds — the folder itself or a subfolder of
///     a tree the library reads in place — cannot mint a second instance, so
///     it is answered before any sheet with "already imported" and a
///     highlight of the shelf the reader meant;
///   * the tree's OWN root, re-picked, is a reconciliation: the walk runs,
///     new files join the tree as linked books, the logs a removal or a
///     departure wrote are spent by their books coming back, and only a walk
///     that found NOTHING raises the note — and a tree that is itself a
///     member standing outside another goes back inside it on the run's
///     answer, because an import of a folder is the reader wanting it home;
///   * a rung the family's ledger names but no shelf wears — deleted, or
///     departed as a copy — imports BACK INTO THE FAMILY: the walk runs on
///     the pick's own ledger, and the run's last act folds the shelf it
///     minted onto the rung its directory names (`RootPlan::fold`), lit
///     where it stands again. Importing a subfolder of a tree is how a
///     deleted rung comes back, not how a second door is made;
///   * a tree whose root rung a hand took OUT — the shelf's own departure —
///     arrives with no shelf and no family slot to name, and simply walks:
///     the copy the departure made wears the level's counter name and left
///     the folder's own name free, so the run re-mints the tree on the seats
///     the disk names and lights what came back.
///
/// A STORED arrival asks the family nothing: copies are the library's own
/// second instance, unrelated to any tree, and the only question they have is
/// the name one at the level they land on — the ordinary sheet, whose answers
/// for a stored arrival are the level's own three (go and look at the shelf
/// that is here, replace it, or a shelf of the next free name). The sheet
/// still withholds *as new* from a read-at-place arrival of a DIFFERENT
/// folder's name, because as new of a linked folder is exactly the second
/// instance the gate exists to prevent.
pub fn import_folder(state: AppState, root: String, opts: FolderOpts) {
    if opts.in_place {
        if let Some((rel, shelf_id, shelf_name)) = covered_shelf(state, &root) {
            if !rel.is_empty() {
                // Ground a family tree already holds: a sentence and a
                // highlight, because a second instance of it is a second door
                // on one folder.
                conflict::raise_note(state, shelf_id, shelf_name, NoteKind::Gated);
                return;
            }
            // The tree's OWN root re-picked: the continuation walk. And a
            // tree standing outside a family that could hold it goes home on
            // the run's answer — the same fold the subfolder pick below gets
            // from the gate, promised here rather than asked, because the
            // reader just said which folder they meant.
            let folders = state.library.folders.get_untracked();
            let fold = folders
                .iter()
                .find(|f| f.root == root)
                .and_then(|f| {
                    let shelves = state.library.shelves.get_untracked();
                    shelves_ops::family_for(&folders, &shelves, &f.root)
                });
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    continuation: Some((shelf_id, shelf_name)),
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
        // Not covered — but the ground may still be a family's: the rung the
        // pick names was deleted, or departed as a copy, and the ledger that
        // covers the ground still stands. An import of a folder is the reader
        // wanting it BACK, and back is the rung its directory names in the
        // tree that covers it rather than a second shelf at the top of the
        // library: the walk runs on the pick's own ledger — where the
        // departed books' logs are, to be spent — and the run's last act
        // folds the shelf into the family.
        let fold = {
            let folders = state.library.folders.get_untracked();
            let shelves = state.library.shelves.get_untracked();
            shelves_ops::family_for(&folders, &shelves, &root)
        };
        if fold.is_some() {
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
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

/// A member of this tree that is standing outside it.
///
/// The shape this answers, and it is one the reader makes rather than a bug:
/// a rung of a watched tree is removed, which cuts the folder's pointer to it
/// and leaves the folder watching; that same subfolder is then imported on its
/// own, which is ground no standing shelf covers, so it becomes a watched
/// folder of its own with a shelf wherever the import put it — usually the
/// top level. A re-import of the OUTER tree then finds every book already
/// standing and has nothing to say, while the shelf they are standing on is
/// not one of its own.
///
/// So the question is asked of the walk's own findings rather than of the shelf
/// list alone: another in-place folder whose root is a subfolder of this one,
/// whose root shelf still stands, which this walk actually found files under,
/// and which is not already hanging inside this tree — a shelf the reader
/// carried in by hand is where the reader put it and is none of this run's
/// business. The shallowest such member answers, because a deeper one hangs
/// under it and comes back with it.
///
/// An explicit run FOLDS the member it finds — an import is an ask, and a
/// member outside its family is an ask answered (`run_fold`) — rather than
/// asking a second question about it.
///
/// Answers the nested folder's id, the rung key its root directory names in
/// this tree, and the shelf's own id and name — the facts the fold runs on
/// and the report speaks.
fn displaced_member(
    state: AppState,
    folder: &WatchedFolder,
    found: &[FoundFile],
) -> Option<DisplacedMember> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let mut best: Option<(usize, DisplacedMember)> = None;
    for other in folders.iter() {
        if other.id == folder.id || !other.opts.in_place {
            continue;
        }
        let Some(rel) = rel_under(&other.root, &folder.root).filter(|rel| !rel.is_empty()) else {
            continue;
        };
        if !found
            .iter()
            .any(|file| rel_under(&file.path, &other.root).is_some())
        {
            continue;
        }
        // The folder's own root shelf, by its map first and by its kind second:
        // the map is what its walk files onto, and a map that lost the pointer
        // still leaves a shelf the folder owns. A folder with neither has no
        // standing shelf to ask about, and is not this run's question — it is
        // the next folder's turn rather than the end of the walk.
        let Some(shelf_id) = other
            .shelf_map
            .get("")
            .filter(|id| shelves.iter().any(|s| &s.id == *id))
            .cloned()
            .or_else(|| root_shelf_of(&shelves, &other.id))
        else {
            continue;
        };
        let Some(shelf) = shelves_ops::find(&shelves, &shelf_id) else {
            continue;
        };
        if shelf.kind.folder_id() == Some(folder.id.as_str())
            || hangs_inside(&shelves, &shelf_id, &folder.id)
        {
            continue;
        }
        let depth = rel.matches('/').count();
        if best.as_ref().is_none_or(|(seen, _)| depth < *seen) {
            best = Some((
                depth,
                DisplacedMember {
                    folder_id: other.id.clone(),
                    rel,
                    shelf_id,
                    shelf_name: shelf.name.clone(),
                },
            ));
        }
    }
    best.map(|(_, member)| member)
}

/// The facts a displaced member's note speaks and its answer writes.
struct DisplacedMember {
    folder_id: String,
    rel: String,
    shelf_id: String,
    shelf_name: String,
}

/// Whether a shelf hangs inside a folder's tree: any ancestor of it is a shelf
/// that folder owns. Bounded by the list rather than by the walk finding its
/// own tail, for the reason `shelf::can_nest` bounds itself — a blob that
/// already carries a cycle answers "no" rather than spinning.
fn hangs_inside(shelves: &[Shelf], shelf_id: &str, folder_id: &str) -> bool {
    let mut current = shelves_ops::find(shelves, shelf_id).and_then(|s| s.parent.clone());
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return false;
        };
        let Some(parent) = shelves_ops::find(shelves, &id) else {
            return false;
        };
        if parent.kind.folder_id() == Some(folder_id) {
            return true;
        }
        current = parent.parent.clone();
    }
    false
}

/// Put a displaced member back on the rung its directory names, and fold the
/// folder that was reading it into the tree that contains it.
///
/// The move the note's second answer owes, and the whole of it: one ground is
/// read by one folder from here on. Three writes, in the order that keeps them
/// honest —
///
///   * the tree's OWN chain down to the rung above the returning one, minted
///     with the walk's own arithmetic, because a reader who removed the rung
///     may have removed the one above it too and a shelf hanging on nothing
///     renders nowhere;
///   * the nested folder's shelves, each taking the tree's `rel` for the rung
///     it stands on — its root becomes the rung itself and its own subfolders
///     become rungs under that, so the disk's tree and the shelf's tree agree
///     again and the next re-hang has nothing to undo;
///   * the nested folder's ledger into the tree's, and the folder itself out.
///     Its `placed` set and its removals go with the ground: a fingerprint the
///     tree did not know it placed is a book its next rescan adds again, and a
///     removal it does not hold is a book its next rescan resurrects. Retiring
///     the folder without them would trade a duplicate shelf for a duplicated
///     library.
///
/// A shelf the reader hand-placed keeps its place through the move and is
/// marked as the reader's, which is what tells the next re-hang to pass it by:
/// the disk names its rung, and the hand still beats the disk. A nesting the
/// graph refuses — a blob that already carries a cycle — leaves the shelf where
/// it hangs for the same reason. A tree being walked right now refuses the move
/// outright: its run holds a clone of the ledger and writes it back whole, so a
/// fold made underneath it would be a fold that never happened.
pub(crate) fn reclaim_rung(
    state: AppState,
    tree_id: &str,
    gone_id: &str,
    rel: &str,
    shelf_id: &str,
) -> bool {
    let now = now_ms();
    let mut minted: Vec<Shelf> = Vec::new();
    let Some((mut tree, gone)) = state.library.folders.with_untracked(|folders| {
        let tree = folder_ops::find(folders, tree_id)?.clone();
        let gone = folder_ops::find(folders, gone_id)?.clone();
        Some((tree, gone))
    }) else {
        return false;
    };
    // Every shelf the nested folder owns, with the rung key it takes in the
    // tree: its root IS the rung, and a subfolder of it hangs under that. Read
    // before anything is written, because the answer is about the shelves that
    // are standing and not about the ones this move is going to mint.
    let rungs: Vec<(String, String)> = state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.kind.folder_id() == Some(gone_id))
            .filter_map(|s| {
                let ShelfKind::Folder { rel: own, .. } = &s.kind else {
                    return None;
                };
                let own = own.clone().unwrap_or_default();
                Some((
                    s.id.clone(),
                    if own.is_empty() {
                        rel.to_string()
                    } else {
                        format!("{rel}/{own}")
                    },
                ))
            })
            .collect()
    });
    // The shelf the note named is the one the answer is about: a shelf that
    // went while the note was up is an answer with nothing to move, and the
    // folder stays the folder it was rather than being folded away for nothing.
    // So does a tree that is being walked right now — its run holds a clone of
    // the ledger and writes it back whole at the end, which would drop
    // everything this fold put in it.
    if !rungs.iter().any(|(id, _)| id == shelf_id) || root_is_claimed(&tree.root) {
        return false;
    }
    // The tree's own rung above the returning one, minted whole: `key_chain`
    // walks from the tree's root shelf down, reusing the rungs the map holds
    // and reporting the ones it has to make.
    let root = tree.root.clone();
    let parent = tree.shelf_chain_for(
        library_core::folder::parent_key(rel).unwrap_or(""),
        |_| id::next_shelf_id(now),
        |rung| shelf_name(rung, &root),
        |rung, id, name, parent| {
            minted.push(Shelf {
                id: id.to_string(),
                name,
                kind: ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(rung),
                },
                books: Vec::new(),
                parent,
                manual_parent: false,
            });
        },
    );
    for (id, key) in &rungs {
        tree.shelf_map.insert(key.clone(), id.clone());
    }
    // The ledger follows the ground.
    tree.placed.extend(gone.placed.iter().copied());
    for stone in gone.ignored.iter() {
        if !tree.is_ignored(&stone.fp) {
            tree.ignored.push(stone.clone());
        }
    }
    tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);

    state.library.shelves.update(|shelves| {
        for shelf in minted {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        let nestable = shelves_ops::can_nest(shelves, shelf_id, &parent);
        for (id, key) in &rungs {
            let Some(shelf) = shelves_ops::find_mut(shelves, id) else {
                continue;
            };
            shelf.kind = ShelfKind::Folder {
                folder_id: tree_id.to_string(),
                rel: rel_of(key),
            };
            if id != shelf_id {
                continue;
            }
            // The returning rung takes the place the disk names, unless the
            // reader's hand put it somewhere the graph will not undo.
            if nestable {
                shelf.parent = Some(parent.clone());
                shelf.manual_parent = false;
            } else {
                shelf.manual_parent = true;
            }
        }
    });
    state.library.folders.update(|folders| {
        folders.retain(|f| f.id != gone_id);
        match folders.iter().position(|f| f.id == tree_id) {
            Some(at) => folders[at] = tree,
            None => folders.push(tree),
        }
    });
    crate::storage::persist_library(state.library);
    true
}

/// The stored arrival's *replace* over a shelf that is NOT the folder's own
/// in-place tree: the books the level's shelf holds leave the library through
/// the removal's own sweep — rows, memberships, covers, highlights, and the
/// store copies the app made — and the folder's copies take the shelf, the
/// walk filing into it as the merge's plan does.
///
/// The claim is asked FIRST, the ordering `replace_folder_with_copies` gives
/// for the tree's own replace: a walk already in flight refuses the run
/// before anything is removed, because a removal no import re-lands is the
/// one outcome this ordering exists to prevent.
pub(crate) fn replace_shelf_with_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    existing_id: String,
) {
    if root_is_claimed(&root) {
        already_importing(state, &root);
        return;
    }
    let doomed: Vec<String> = {
        let (rows, shelves) = state.library.snapshot_rows();
        shelves_ops::members_of(&rows, &shelves, &existing_id)
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    if !doomed.is_empty() {
        super::arrange::purge_books(state, &doomed, PurgeOpts::default());
    }
    proceed_folder(
        state,
        root,
        opts,
        RootPlan {
            into: Some(existing_id),
            ..Default::default()
        },
    );
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
        // A copies run's file is an add BECAUSE the library holds the
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

/// The library as one folder run sees it: the snapshot the ledger diffs against,
/// the walk's own answer, and the set of addresses the run owes a copy of its
/// own.
///
/// A value so the stages below read one consistent picture rather than each
/// taking its own borrow of four locals. Nothing in it is written back — what
/// lands is applied to the LIVE signals at the end, so a book the reader opened
/// while the walk was running is not overwritten by one.
struct Snapshot<'a> {
    books: &'a [Row],
    registry: &'a ledger::Registry,
    found: &'a [FoundFile],
    copy_paths: &'a HashSet<String>,
}

/// The folder's ledger row for this run.
///
/// Importing a folder the library already watches continues that row's `placed`
/// and `ignored` sets, which is the whole point of them: re-importing is how a
/// reader would otherwise get back every book they deleted last week. A folder
/// the library has never seen is minted here, and a minted row carries no
/// history for any rule to misread.
fn resolve_folder(
    folders: &[WatchedFolder],
    root: &str,
    opts: FolderOpts,
    plan: &RootPlan,
) -> WatchedFolder {
    let standing = folders.iter().find(|f| f.root == root);
    let mut folder = standing.cloned().unwrap_or_else(|| WatchedFolder {
        id: id::next_folder_id(now_ms()),
        root: root.to_string(),
        opts: opts.clone(),
        placed: HashSet::new(),
        ignored: Vec::new(),
        shelf_map: BTreeMap::new(),
        last_seen: Vec::new(),
        scanned_ms: 0,
    });
    // The sheet's answers are this import's truth, and the next scan's.
    folder.opts = opts;
    // A merge files into the shelf the level already held: the folder's root
    // rung is that shelf, and the map is the one place the walk, the chain
    // minting and every later rescan read the answer from — which is what makes
    // the merge a promise the next scan keeps.
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
    folder
}

/// Hang this folder's shelves back on the rungs their `rel` names.
///
/// The tree on disk is the tree on the shelf, for the shelves this folder owns —
/// the rule and its edge cases (a hand-moved shelf keeps its place, a subtree
/// re-hangs together, a virtual shelf is never touched) are
/// `library_core::shelf::rehang_moves`', pure and host-tested; this is the one
/// pass that asks it and applies the answer.
///
/// Before the diff and before the "nothing changed" return on purpose: a library
/// arranged by an older build is repaired by the first rescan that looks at the
/// folder, not only by an import that happens to add something.
fn rehang(state: AppState, folder_id: &str) {
    let moves = state
        .library
        .shelves
        .with_untracked(|shelves| shelves_ops::rehang_moves(shelves, folder_id));
    if moves.is_empty() {
        return;
    }
    state.library.shelves.update(|shelves| {
        for (id, want) in &moves {
            if let Some(shelf) = shelves_ops::find_mut(shelves, id) {
                shelf.parent = want.clone();
            }
        }
    });
    crate::storage::persist_library(state.library);
}

/// Take the REPRESENTED files out of the walk, and answer with the rows they are
/// represented by.
///
/// A file the folder's log says is represented — a moved-out log bound to the
/// stored copy that came home — is an import that succeeds by counting the row
/// the log names, not by minting a linked neighbour beside the copy the reader
/// already moved back. The copy is never the light's target: it is a book of
/// its own, and what a FOLDER import reveals is the folder — the run's own
/// shelf, lit on the level that holds it. The log stays standing: it is the
/// folder's permanent word that this file has a row, and a rescan stays silent
/// about it as it always was.
///
/// Explicit runs only, and the caller is what knows that: a rescan never reveals,
/// so it never asks. A binding that names a dead row is spent of its meaning, and
/// the file stays in the walk to take the ordinary import route, which lifts the
/// log when the book lands.
fn split_represented(
    state: AppState,
    folder: &WatchedFolder,
    found: &mut Vec<FoundFile>,
) -> Vec<String> {
    let mut represented = Vec::new();
    found.retain(|file| {
        let Some(row_id) = ledger::find_tombstone(folder, &file.fp)
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
            true
        }
    });
    represented
}

/// The placements a PLANNED tree owes the files the library already holds, and
/// the per-file questions a standing rung's name asks on the way.
///
/// A planned tree — the folder sheet's *as new* or *merge* answer — owes a
/// placement for EVERY file the walk found that the library already holds: a
/// membership of the row it holds it in, never a second row, because one content
/// is one identity and one identity is one row. Those files are exactly the ones
/// the ledger's table answers with a Skip, so a tree promised as "its own shelf,
/// its own tree" would otherwise hold only the new files — and a re-import of one
/// folder, whose every file is known, would hold nothing at all.
///
/// A merge asks before it places: a file whose name a STANDING rung holds — the
/// root shelf the answer named, or any subfolder shelf a previous run mapped — is
/// the compact sheet's per-file question, even when the row wearing the name is
/// the row the file resolves to, because a reader who re-imports a folder to
/// reconcile it is owed the three answers per file wherever the names meet, not a
/// card that says nothing was new. Files no standing name collides with join
/// silently: new books in a merged folder are the default, not a case.
fn planned_placements(
    state: AppState,
    snap: &Snapshot<'_>,
    folder: &WatchedFolder,
    plan: &RootPlan,
    adds: &mut Vec<FoundFile>,
) -> (Vec<(String, FoundFile)>, Vec<ConflictAsk>) {
    if plan.rename.is_none() && plan.into.is_none() {
        return (Vec::new(), Vec::new());
    }
    // Known content never reaches a planned run's add list: its placement is the
    // membership below, and an add would either resolve to the same row twice or
    // — in a copying folder — make a store copy nothing reads. The copies run's
    // own files are the exception the rule exists for: a second instance the
    // reader just asked for, of a file a tree keeps reading in place.
    adds.retain(|f| {
        !snap.registry.contains_key(&f.fp) || snap.copy_paths.contains(&f.path)
    });
    let shelves_now = state.library.shelves.get_untracked();
    let mut replacements = Vec::new();
    let mut asks = Vec::new();
    for file in snap.found {
        // The row the library holds this file in: by content identity first (the
        // ledger's own answer), and by address second for the migrated row whose
        // placeholder identity no measurement matched.
        let known = snap
            .registry
            .get(&file.fp)
            .map(|k| k.id.clone())
            .or_else(|| {
                book_rows(snap.books)
                    .find(|b| b.path() == file.path)
                    .map(|b| b.id.clone())
            });
        let Some(row_id) = known else {
            continue;
        };
        // A file the copies run owes lands as a book of its own below, not as
        // a membership of the row it duplicates.
        if snap.copy_paths.contains(&file.path) {
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
                library_core::conflict::collide(snap.books, &shelves_now, &arrival)
            {
                let existing_name =
                    conflict::existing_name_of(snap.books, &existing_id, &arrival);
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
    (replacements, asks)
}

/// The compact sheet's questions over the walk's genuinely NEW files.
///
/// The same question [`planned_placements`] asks of the files the library already
/// held, asked of the ones it did not: a merge's new arrivals land on a rung that
/// may already hold their NAME, and a collision there is the compact sheet's too
/// (one book / replace / as new, one at a time or one answer for all). Everything
/// else goes in without being asked — new books are the default, not a case. A new
/// file inside a SUBFOLDER a previous run mapped is asked against that subfolder's
/// shelf; a file in a rung being minted fresh has nothing standing to collide with.
///
/// Only a merge asks. An *as new* tree mints every rung it files into, so there is
/// nothing standing for a name to collide with.
fn screen_merge_adds(
    state: AppState,
    snap: &Snapshot<'_>,
    folder: &WatchedFolder,
    plan: &RootPlan,
    adds: &mut Vec<FoundFile>,
) -> Vec<ConflictAsk> {
    let Some(into) = plan.into.clone() else {
        return Vec::new();
    };
    let shelves_now = state.library.shelves.get_untracked();
    let mut asks = Vec::new();
    adds.retain(|file| {
        let key = folder.shelf_key(file);
        let target = if key.is_empty() {
            Some(into.clone())
        } else {
            folder.shelf_map.get(&key).cloned()
        };
        let Some(target) = target else {
            return true;
        };
        let arrival = Arrival::import(file.clone(), target, None);
        match library_core::conflict::collide(snap.books, &shelves_now, &arrival) {
            Some(existing_id) => {
                let existing_name = conflict::existing_name_of(snap.books, &existing_id, &arrival);
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
    asks
}

/// What one walked file needs in order to become a row: the copies that landed,
/// the measurements of the copies run's own, and the shape of the tree to mint
/// into.
///
/// A value rather than eight arguments, because every one of them is a fact about
/// the RUN and not about the file — the file supplies its own address, its
/// measurement and the id minted for it, and everything else is the same for all
/// four hundred of them.
struct Landing<'a> {
    copies: &'a HashMap<String, String>,
    copy_measured: &'a HashMap<String, Fingerprint>,
    copy_paths: &'a HashSet<String>,
    planned_name: &'a Option<String>,
    root: &'a str,
    in_place: bool,
    merged: bool,
    now: u64,
}

/// What minting one walked file produced.
enum Minted {
    /// A row was placed: its id, and the shelf the chain minted for it.
    Placed { id: String, shelf: String },
    /// The address the file was found at already belonged to a row, so the row
    /// was measured rather than duplicated beside its own twin.
    Healed,
    /// The copy the options owed did not land, so there are no bytes to read and
    /// no row to promise.
    CopyFailed,
}

/// Mint one walked file's row on the LIVE list, or heal the row that appeared at
/// its address while the walk was running.
///
/// The three outcomes are the three things that can be true of a file a walk
/// found, and telling them apart is the run's whole job:
///
///   * a row already reads this address, so the file IS that book and the walk has
///     just made the measurement the startup pass could not. This is the second
///     half of the heal-by-address rule, for the book that appeared while the walk
///     was running. Membership is left alone — the ledger records the placement,
///     and where the reader filed it is the reader's business. The copies run's
///     own files are exempt by name: their twin IS the point of them;
///   * the copy the options owed failed, so there are no bytes to read and a book
///     here would be a card that opens onto an error. The failure is already on
///     the toast the batch raised;
///   * otherwise the file is a new book, and it is minted from the walk's own
///     measurement — a real fingerprint, not a placeholder, so nothing about it is
///     pending.
///
/// A file this folder remembers REMOVING is coming back on an explicit ask, and
/// the reader should not notice it was ever gone: the row returns wearing the name
/// the shelf showed, and the ledger's log is spent by the landing. A copy that
/// fails leaves the log standing, which is the one honest outcome for a file that
/// could not be filed — so the spend happens here, with the landing, and not with
/// the diff.
fn mint_walked_row(
    books: &mut Vec<Row>,
    folder: &mut WatchedFolder,
    landing: &Landing<'_>,
    book_id: String,
    file: &FoundFile,
    new_shelves: &mut Vec<Shelf>,
) -> Minted {
    let own_copy = landing.copy_paths.contains(&file.path);
    if !own_copy
        && let Some(existing) = book_rows_mut(books).find(|b| b.path() == file.path)
    {
        existing.heal(file.fp);
        folder.mark_placed(file.fp);
        return Minted::Healed;
    }
    let origin = if landing.in_place {
        Origin::Linked {
            src: file.path.clone(),
        }
    } else {
        let Some(store) = landing.copies.get(&book_id) else {
            return Minted::CopyFailed;
        };
        Origin::Stored {
            src: Some(file.path.clone()),
            store: store.clone(),
        }
    };
    let stone = ledger::find_tombstone(folder, &file.fp).cloned();
    let store_at = match &origin {
        Origin::Stored { store, .. } => Some(store.clone()),
        Origin::Linked { .. } => None,
    };
    let mut book = Book::new(
        book_id,
        file.fp,
        file.format().unwrap_or(Format::Pdf),
        origin,
        landing.now,
    );
    if let Some(title) = stone.as_ref().and_then(|s| s.title.clone()) {
        book.title = Some(title);
    }
    // The copy list's file is a book of its own beside the linked book the
    // tree keeps: independent, so its marks and its place in it are its own,
    // and known by its copy's measurement — or by the pending flag the startup
    // sweep finishes, when the copy could not be weighed. `add_book`'s
    // one-row-per-fingerprint rule is the right rule for a walk and the wrong
    // one for a second instance the reader just asked for by name, so the copy
    // is pushed past it.
    //
    // A read-at-place book coming back beside the library's copy of ITS OWN
    // file is pushed past the same rule, and for the same shape of reason: one
    // content, two rows, each with one address. The copy wears the fingerprint
    // on a host that stamps a copy like its source, so `add_book` would answer
    // with the copy's id — and the walk would then file the COPY on this rung
    // and report a book come home that never left the store. The link the
    // folder reads and the copy the reader moved out are the shape a departure
    // leaves behind on purpose, so the link is minted as its own row. A
    // COPYING folder keeps the rule whole for the files it already copied: a
    // second copy of a content the library holds as its OWN is the orphan in
    // the store the rule exists to prevent — the copies this run owes are of
    // files the library READS, and they come through the copy list above.
    let beside_its_own_copy = landing.in_place
        && book_rows(books).any(|b| {
            !b.independent && b.fp == file.fp && b.origin.is_store_copy_of(&file.path)
        });
    let placed_id = if own_copy {
        book.independent = true;
        book.adopt_measurement(
            store_at
                .as_ref()
                .and_then(|store| landing.copy_measured.get(store))
                .copied(),
        );
        let id = book.id.clone();
        books.push(Row::Book(book));
        id
    } else if beside_its_own_copy {
        let id = book.id.clone();
        books.push(Row::Book(book));
        id
    } else {
        add_book(books, book)
    };
    // The whole chain, not the leaf: importing "1" whose inside is "2", "3" and
    // four books has to produce "1" at the root with "2", "3" and the four books
    // inside it — not three siblings at the root and the books twice.
    let key = folder.shelf_key(file);
    let shelf_id = chain_for(
        folder,
        &key,
        landing.now,
        landing.root,
        landing.planned_name,
        landing.merged,
        new_shelves,
    );
    folder.mark_placed(file.fp);
    // The book landed, so a removal that was holding it out is spent.
    ledger::restore_deleted(folder, &file.fp);
    Minted::Placed {
        id: placed_id,
        shelf: shelf_id,
    }
}

/// Scan one folder, run the ledger over what the walk found, copy whatever the
/// options say to copy, and write the result in one go.
///
/// Five stages, and this function is the order they run in rather than any of
/// them: [`resolve_folder`] says which ledger row the walk continues,
/// [`rehang`] puts the folder's shelves back on the
/// rungs their directories name, the diff decides what the walk owes
/// ([`ledger::diff_folder`] for a rescan and [`ledger::diff_import`] for a run the
/// reader asked for, which differ about tombstones and nothing else), the copy
/// batch makes whatever bytes the options say to make, and the write at the end
/// lands all of it on the live signals at once.
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

    let mut folder = resolve_folder(&folders, &root, opts, &plan);
    rehang(state, &folder.id);

    let registry = ledger::registry_of(&books);

    // The copy list, read off the same snapshot the diff reads and before the
    // run writes anything: the addresses whose file the library already reads
    // IN PLACE, and where this run lands a copy of its own beside the linked
    // row. An explicit COPIES run owes every one of them a book — a copies
    // import is the library's own second instance, unrelated to the tree that
    // reads the ground, and a walk that answered Skip over all of them is the
    // silent "Imported 0 books" this list exists to prevent. A RESCAN never
    // owes one: staying quiet about ground another folder placed is the
    // rescan's whole job, and the copies a previous import made are known by
    // their own bytes rather than by these addresses.
    let copy_paths: HashSet<String> = if !folder.opts.in_place && !quiet {
        ledger::copy_over_paths(&found, &registry, &books)
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

    // Explicit runs only — a rescan never reveals, so it never asks.
    let represented: Vec<String> = if quiet {
        Vec::new()
    } else {
        split_represented(state, &folder, &mut found)
    };

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
    // is a relink of the WRONG row — the ledger owns that rule and its test.
    ledger::keep_healable_relinks(&mut relinks, &books);
    let relinked = relinks.len();

    // The ledger answered Skip for the copy run's own files — their content is
    // known — but the run owes each of them a book of its own: back onto the
    // add list they go, and the planned-tree pass below keeps them out of the
    // memberships it owes the OTHER known files.
    if !copy_paths.is_empty() {
        for file in found
            .iter()
            .filter(|f| copy_paths.contains(&f.path))
        {
            if !adds.iter().any(|a| a.path == file.path) {
                adds.push(file.clone());
            }
        }
    }

    // The snapshot every stage below reads, so they all decide against one
    // picture of the library rather than each taking its own borrow of four
    // locals. Dropped by the heal underneath it, which writes to `books`.
    let (replacements, mut asks) = {
        let snap = Snapshot {
            books: &books,
            registry: &registry,
            found: &found,
            copy_paths: &copy_paths,
        };
        planned_placements(state, &snap, &folder, &plan, &mut adds)
    };

    // A file at an address the library already holds IS that book, whatever the
    // two fingerprints say. The case this catches is a row migrated from the
    // previous schema: it carries a placeholder identity because nothing ever
    // measured it, so the ledger above saw "unknown content" — and adding it
    // would put a second copy of the same file on the shelf next to its own
    // twin. Healing the row is the honest answer, and the walk has just made the
    // measurement the startup pass could not.
    let mut healed = heal_by_address(&mut books, &mut adds, &copy_paths);

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

    // The same question for the files the ledger has NOT seen, which a merge
    // asks and an *as new* tree has nothing standing to ask it against.
    {
        let snap = Snapshot {
            books: &books,
            registry: &registry,
            found: &found,
            copy_paths: &copy_paths,
        };
        asks.extend(screen_merge_adds(state, &snap, &folder, &plan, &mut adds));
    }

    if adds.is_empty()
        && relinked == 0
        && healed == 0
        && asks.is_empty()
        && replacements.is_empty()
        && represented.is_empty()
    {
        // Nothing to do. A quiet run leaves no trace beyond the folder's own
        // "last scanned" stamp; an explicit import still owes the reader an
        // answer, which is a card saying nothing was new — and a re-pick of a
        // tree the library already reads in place owes the note as well, the
        // gate's old sentence earned now by a walk that found every book
        // already standing.
        //
        // One shape of "nothing new" is not the note's, and it is an ANSWER
        // rather than a question: a member of this tree is standing OUTSIDE
        // it — its rung was removed and the subfolder imported on its own, or
        // the pick itself is a folder a family could hold. An import is an
        // ask, and a member outside its family is an ask answered: the fold
        // puts it back on the rung its directory names, and the report names
        // the shelf that went home.
        folder.scanned_ms = now_ms();
        let root_rung = folder.shelf_map.get("").cloned();
        let folder_id = folder.id.clone();
        write_folder(state, folder);
        if !quiet {
            let folded = state
                .library
                .folder(&folder_id)
                .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found));
            match folded {
                Some((shelf_id, name)) => {
                    conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
                }
                None => {
                    if let Some((shelf_id, name)) = plan.continuation.clone() {
                        conflict::raise_note(state, shelf_id, name, NoteKind::NothingNew);
                    }
                }
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
    let expected = (pending.len() + relinked + healed + replaced) as u32;
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
    // The copy run's copies are measured in one pass before a row is promised:
    // an independent copy of a file a tree still reads in place must not wear
    // the ORIGINAL's fingerprint — that identity stays the linked book's, and
    // the copy is known by its own bytes, the way every instance the library
    // owns is.
    let copy_measured: HashMap<String, Fingerprint> = if !copy_paths.is_empty() {
        let stores: Vec<String> = pending
            .iter()
            .filter(|(_, file)| copy_paths.contains(&file.path))
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
    let in_place = folder.opts.in_place;
    let planned_name = plan.rename.clone();
    let merged = plan.into.is_some();
    let landing = Landing {
        copies: &copies,
        copy_measured: &copy_measured,
        copy_paths: &copy_paths,
        planned_name: &planned_name,
        root: &root,
        in_place,
        merged,
        now,
    };

    state.library.books.update(|books| {
        for (book_id, to) in relinks {
            if ledger::relink(books, &book_id, &to) {
                relink_count += 1;
            }
        }
        for (book_id, file) in pending {
            match mint_walked_row(books, &mut folder, &landing, book_id, file, &mut new_shelves) {
                Minted::Placed { id, shelf } => {
                    placements.push((id, shelf));
                    placed += 1;
                }
                Minted::Healed => healed += 1,
                Minted::CopyFailed => {}
            }
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
                landing.now,
                landing.root,
                landing.planned_name,
                landing.merged,
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
    let root_rung = folder.shelf_map.get("").cloned();
    let folder_id = folder.id.clone();
    write_folder(state, folder);
    crate::storage::persist_library(state.library);
    // A folder import is the case this matters most: it is the one way a shelf
    // arrives with dozens of books at once, and a plate of fallbacks is not a
    // shelf the reader can scan.
    super::covers::backfill_missing(state);

    // The fold an explicit run owes at its end — the plan's own, which puts
    // the picked folder back into the family its ground names, or the outer
    // tree's, which puts a member found standing outside back inside — runs
    // before any light, so what is revealed is the shelf WHERE IT NOW IS.
    let folded = if quiet {
        None
    } else {
        state
            .library
            .folder(&folder_id)
            .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found))
    };

    // What a FOLDER import reveals is the folder: the run's own root shelf,
    // lit on the level that holds it — the reader stays outside, where the
    // folder is visible, because a folder import is about a folder. Going
    // inside and lighting a book is what a FILE import does (`run_files`),
    // and a folder's books are not the folder. A fold gets the sentence
    // instead of the bare light: the note says the shelf went home, and its
    // highlight rides the note's close like every note's.
    let represented_count = represented.len() as u32;
    if !quiet {
        match folded {
            Some((shelf_id, name)) => {
                conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
            }
            None => {
                if let Some(rung) = &root_rung {
                    super::reveal::reveal_shelf(state, rung);
                }
            }
        }
    }

    // The asks are raised after the clean half landed and the blob was
    // written: the sheet counts against the level as the landing left it, and
    // a card that finishes with questions outstanding says so rather than
    // claiming an import nobody has answered yet.
    let waiting = asks.len() as u32;
    if !asks.is_empty() {
        conflict::raise(state, asks);
    }

    let total = placed + (relink_count + healed + replaced) as u32 + represented_count;
    finish_task(state, &task, total, waiting);
}

/// The fold an explicit run owes at its end, and the shelf it seated.
///
/// The plan's own fold first: the run walked the picked folder's ledger, and
/// its root shelf is the member going home — an import of a folder a family
/// tree covers is the reader wanting it back on the rung its directory names,
/// which is `reclaim_rung`'s own arithmetic and not a second one. With no
/// fold planned, the run asks whether a member of the tree it just walked is
/// standing OUTSIDE it — the shallowest, because a deeper one hangs under it
/// and comes back with it — and puts that home: an import is an ask, and a
/// member outside its family is an ask answered rather than a second question.
///
/// `None` when nothing folded: a rescan, which never asks and never moves; a
/// tree with no member outside it; or a fold the guards refused — the family
/// being walked by a run of its own, or the shelf gone while this one ran. A
/// refused fold is not news: the member stands where it stood, and the next
/// explicit import answers again.
fn run_fold(
    state: AppState,
    plan: &RootPlan,
    folder: &WatchedFolder,
    root_rung: Option<&str>,
    found: &[FoundFile],
) -> Option<(String, String)> {
    if let Some((tree, rel)) = &plan.fold {
        let rung = root_rung?;
        reclaim_rung(state, tree, &folder.id, rel, rung)
            .then(|| (rung.to_string(), state.library.shelf_name(rung)))
    } else {
        let member = displaced_member(state, folder, found)?;
        reclaim_rung(
            state,
            &folder.id,
            &member.folder_id,
            &member.rel,
            &member.shelf_id,
        )
        .then(|| (member.shelf_id.clone(), member.shelf_name.clone()))
    }
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
/// One spelling for both batches the library copies — a folder walk's and a
/// loose file drop's — because a per-file failure is the same news either way
/// and the reader should hear it in the same words. `noun` is the only thing
/// that differs and it is what the sentence counts: the files on the way in.
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

/// The rows a *replace* of the folder's own read-at-place tree would take
/// out: the folder's own linked books. A stored book on one of its shelves —
/// a copy that came home — is NOT among them: the replace is about the
/// instances that read the OS folder, and a copy the library already owns is
/// exactly what the shelf ends up holding.
pub fn replace_rows_of_tree(state: AppState, root: &str) -> Vec<String> {
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
        .with_untracked(|rows| ledger::linked_rows_of(rows, &placed))
}

/// The replace answer's first half: the folder's linked books leave the
/// library through the removal's own sweep — row, memberships, cover,
/// highlights, and a tombstone per book in the folder's ledger. The copy
/// import that follows spends those logs as it lands, so the shelf comes
/// back holding only the library's copies, in the names the shelves showed.
pub(crate) fn purge_folder_linked_books(state: AppState, root: &str) {
    let doomed = replace_rows_of_tree(state, root);
    if !doomed.is_empty() {
        super::arrange::purge_books(state, &doomed, PurgeOpts::default());
    }
}

/// The *replace* of the folder's own read-at-place tree, whole: the linked
/// books go through the sweep, and the folder walks again as the copies the
/// reader asked for.
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
    /// A folder that holds the file has an answer for it — a log, or the
    /// folder's own `placed` set standing in for a log an older build dropped —
    /// and an explicit import spends it the way a folder walk does: the book
    /// comes back as the folder's own linked book, in its folder's place,
    /// wearing the name the shelf showed, and the run lights it up. A restore
    /// with no log behind it writes no ledger entry, so it is the folder's rung
    /// rather than a remembered shelf that says where the book comes back to.
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
///
/// The folder's own `placed` set answers third, and only when no log does: a
/// fingerprint this folder placed, with no row at the address and nothing in
/// the ledger, is a book that left and lost its paperwork. Two ways happen —
/// a storage trim dropped the row, and a departure logged by a build whose
/// copy wore the SOURCE's fingerprint, which the next walk's prune then read
/// as a book come back and dropped. Both owe the answer the log would have
/// given, and the alternative is a second stored copy of a file this folder
/// reads in place, landed beside the copy that left. A folder that never
/// placed the file answers `Ordinary` exactly as before: a log is the third
/// arm's evidence, and `placed` is what stands in for one.
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
    if let Some((folder_id, stone)) = stoned {
        return CoveredFate::Restore { folder_id, stone };
    }
    // No row and no log, so the folder's own membership is the evidence: it
    // PLACED this fingerprint, which means a linked book of this file stood
    // here and is not standing now. The name comes from the copy the library
    // holds of this very address when there is one, because a departure moved
    // the shelf's name into it and the book that comes back should wear the
    // name the reader remembers rather than the file's stem.
    let placed_by = state.library.folders.with_untracked(|folders| {
        covering
            .iter()
            .find(|id| {
                folders
                    .iter()
                    .any(|f| &f.id == *id && f.placed.contains(&file.fp))
            })
            .cloned()
    });
    match placed_by {
        Some(folder_id) => {
            let title = state.library.books.with_untracked(|rows| {
                book_rows(rows)
                    .find(|b| b.origin.is_store_copy_of(&file.path))
                    .and_then(|b| b.title.clone())
            });
            CoveredFate::Restore {
                folder_id,
                stone: Tombstone {
                    fp: file.fp,
                    title,
                    format: file.format().unwrap_or(Format::Pdf),
                    last_path: file.path.clone(),
                    // No shelf to remember, so the landing falls through to
                    // the folder's own mapped rung for this file's subfolder —
                    // the ground the book left, which is the answer the log
                    // would have given.
                    shelf_id: None,
                    removed_ms: now_ms(),
                    moved: true,
                    returned_row: None,
                },
            }
        }
        None => CoveredFate::Ordinary,
    }
}

/// The write half of [`CoveredFate::Restore`]: the folder's book comes back
/// the way a folder walk brings it back — a LINKED book at the file's
/// address, wearing the name the shelf showed, on the folder's own ground —
/// and the log is spent by the landing, when there is one to spend. Returns
/// the row so the run can light it up.
///
/// The shelf is the one the log remembers when it still stands, then the
/// folder's mapped rung for the file's subfolder, then the folder's root
/// shelf: a book that came back should not come back somewhere new, and
/// least of all on the level the file happened to be dropped on — the drop
/// asked for a file the folder owns, and the folder's place is the answer.
/// A restore with no log behind it — a departure whose paperwork an older
/// build dropped — has no shelf to remember and starts at the rung, which is
/// the same answer the log would have given. A folder with no shelf left at
/// all leaves the book in the library unfiled, which is the restore menu's
/// own fallback.
fn restore_covered_file(
    state: AppState,
    file: &FoundFile,
    folder_id: &str,
    stone: &Tombstone,
) -> String {
    // The folder's two rungs for this file: the one its subfolder maps to,
    // and the one at its root. The ledger's own arithmetic — the same one that
    // decides whether a book has left the folder's ground — so a book comes
    // back to the rung it departed from rather than to wherever a second copy
    // of that lookup happens to land.
    let (rung, root_rung) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, folder_id)
            .map(|f| {
                let (rung, root) = f.rungs_for(&file.path);
                (rung.map(str::to_string), root.map(str::to_string))
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
        CoveredFate, claim_root, covered_fate, covered_shelf, displaced_member, land_file,
        purge_folder_linked_books, rel_of, replace_rows_of_tree, restore_covered_file, shelf_name,
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

    /// The same file, with the `rel` a walk of `root` would report: the path
    /// under the watched root, which is what the rung key is read from. A test
    /// whose file sits in a subfolder needs the real thing, because a `rel` of
    /// only the file's own name puts every book on the folder's root shelf.
    fn found_under(root: &str, path: &str, n: u32) -> FoundFile {
        let rel = library_core::folder::rel_under(path, root)
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string());
        FoundFile {
            rel,
            ..found(path, n)
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
        let file = found_under("/books", "/books/dune.md", 7);
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
        let file = found_under("/books", "/books/scifi/dune.md", 7);
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
        let file = found_under("/books", "/books/dune.md", 7);
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
        let file = found_under("/books", "/books/dune.md", 7);
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
        // A file the folder never placed is an ordinary import even with the
        // library's own copy of it standing: the copy's provenance is not the
        // folder's membership, and only the folder's ledger can say the book
        // was once its own.
        state.library.books.set(vec![stored("b9", "/books/new.md", "/store/b9.md", 9)]);
        assert!(
            matches!(covered_fate(state, &fresh), CoveredFate::Ordinary),
            "a copy of a file the folder never placed makes it no less ordinary"
        );
    }

    /// The departure whose log an older build dropped.
    ///
    /// A host that stamps a copy like its source left the library holding the
    /// SOURCE's fingerprint on the copy's row, so the next walk's prune read the
    /// moved-out log as a book come back and dropped it. What is left is a folder
    /// that placed the file, no row at its address, and no log — and the answer
    /// is the one the log would have given: the book comes back as the folder's
    /// own linked book, on the folder's own rung, wearing the name the copy
    /// carries, and the copy stays where the reader put it.
    #[test]
    fn a_departure_whose_log_is_gone_still_brings_the_linked_book_back() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found_under("/books", "/books/scifi/dune.md", 7);
        state.library.shelves.set(vec![plain("fs"), plain("mid"), plain("sub")]);
        // The copy the departure made, standing on the middle rung, wearing the
        // source's fingerprint the way a stamp-preserving host leaves it.
        let mut copy = stored("b1", "/books/scifi/dune.md", "/store/b1.md", 7);
        copy.as_book_mut().expect("a book").title = Some("Dune".to_string());
        state.library.books.set(vec![copy]);
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "mid") {
                shelf.books.push("b1".to_string());
            }
        });
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        one.shelf_map.insert("scifi".to_string(), "sub".to_string());
        state.library.folders.set(vec![one]);

        let fate = covered_fate(state, &file);
        assert!(
            matches!(fate, CoveredFate::Restore { .. }),
            "the folder's own membership is the log's stand-in: got {fate:?}"
        );
        let CoveredFate::Restore { folder_id, stone } = fate else {
            unreachable!()
        };
        assert_eq!(folder_id, "f1");
        assert!(stone.moved, "the book left; it was not removed");
        assert_eq!(
            stone.title.as_deref(),
            Some("Dune"),
            "named by the copy that carries the name"
        );
        assert_eq!(stone.shelf_id, None, "with no shelf to remember, the folder's rung answers");

        let id = restore_covered_file(state, &file, &folder_id, &stone);

        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 2, "the link is back beside the copy, and not instead of it");
        let back = rows
            .iter()
            .find(|r| r.id() == id)
            .and_then(|r| r.book())
            .expect("a book row");
        assert!(matches!(back.origin, Origin::Linked { .. }));
        assert_eq!(back.path(), "/books/scifi/dune.md", "reading the folder's file again");
        assert_eq!(back.title.as_deref(), Some("Dune"), "wearing the name the shelf showed");
        assert!(!back.independent, "it is the folder's book, not a private one");
        let copy = rows
            .iter()
            .find(|r| r.id() == "b1")
            .and_then(|r| r.book())
            .expect("the copy");
        assert!(copy.origin.is_stored(), "and the copy the reader moved out is untouched");
        let shelves = state.library.shelves.get_untracked();
        let on = |sid: &str| {
            shelves
                .iter()
                .find(|s| s.id == sid)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            on("sub"),
            vec![id],
            "on the rung the folder names for the file — not the one the copy is on"
        );
        assert_eq!(
            on("mid"),
            vec!["b1".to_string()],
            "and the copy stays where the reader put it"
        );
        assert!(on("fs").is_empty());
    }

    /// The same shape arriving through a folder walk rather than a loose file:
    /// the copy holds the fingerprint the walk measured, so the walk's own
    /// one-row-per-fingerprint rule would answer with the COPY's id and file the
    /// copy on the rung. What the reader asked for is the file's linked book.
    #[test]
    fn a_walk_mints_the_link_beside_the_copy_that_holds_its_fingerprint() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found_under("/books", "/books/dune.md", 7);
        let mut copy = stored("b1", "/books/dune.md", "/store/b1.md", 7);
        copy.as_book_mut().expect("a book").title = Some("Dune".to_string());
        state.library.books.set(vec![copy]);
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);
        state.library.shelves.set(vec![plain("fs")]);

        let mut books = state.library.books.get_untracked();
        let mut folder = state.library.folders.get_untracked().remove(0);
        let empty_copies: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let empty_measured: std::collections::HashMap<String, Fingerprint> = std::collections::HashMap::new();
        let empty_copy_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let planned_name: Option<String> = None;
        let landing = super::Landing {
            copies: &empty_copies,
            copy_measured: &empty_measured,
            copy_paths: &empty_copy_paths,
            planned_name: &planned_name,
            root: "/books",
            in_place: true,
            merged: false,
            now: 1,
        };
        let mut new_shelves = Vec::new();
        let minted = super::mint_walked_row(
            &mut books,
            &mut folder,
            &landing,
            "b2".to_string(),
            &file,
            &mut new_shelves,
        );
        let super::Minted::Placed { id, shelf } = minted else {
            panic!("the file owes a row of its own");
        };
        assert_eq!(id, "b2", "the link is its own row, not the copy's id");
        assert_eq!(shelf, "fs", "filed on the folder's own rung");
        assert_eq!(books.len(), 2, "and the copy is still standing");
        let back = books
            .iter()
            .find(|r| r.id() == "b2")
            .and_then(|r| r.book())
            .expect("a book row");
        assert!(matches!(back.origin, Origin::Linked { .. }));
        assert_eq!(back.path(), "/books/dune.md");
        assert!(!back.independent, "the folder's book is a shared row");
        assert!(folder.placed.contains(&file.fp), "and the folder still answers for the file");
    }

    /// The same walk over a COPYING folder keeps the rule whole: a second copy
    /// of a file the library already copied is the duplicate the one-row rule
    /// exists to prevent, so the arrival resolves to the copy that is there.
    #[test]
    fn a_copying_folder_never_mints_a_second_copy_of_its_own_file() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let file = found_under("/books", "/books/dune.md", 7);
        state.library.books.set(vec![stored("b1", "/books/dune.md", "/store/b1.md", 7)]);
        let mut one = folder("f1", "/books", &[7], Vec::new());
        one.opts.in_place = false;
        one.shelf_map.insert(String::new(), "fs".to_string());
        state.library.folders.set(vec![one]);
        state.library.shelves.set(vec![plain("fs")]);

        let mut books = state.library.books.get_untracked();
        let mut folder = state.library.folders.get_untracked().remove(0);
        let mut copies: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        copies.insert("b2".to_string(), "/store/b2.md".to_string());
        let empty_measured: std::collections::HashMap<String, Fingerprint> = std::collections::HashMap::new();
        let empty_copies: std::collections::HashSet<String> = std::collections::HashSet::new();
        let planned_name: Option<String> = None;
        let landing = super::Landing {
            copies: &copies,
            copy_measured: &empty_measured,
            copy_paths: &empty_copies,
            planned_name: &planned_name,
            root: "/books",
            in_place: false,
            merged: false,
            now: 1,
        };
        let mut new_shelves = Vec::new();
        let minted = super::mint_walked_row(
            &mut books,
            &mut folder,
            &landing,
            "b2".to_string(),
            &file,
            &mut new_shelves,
        );
        let super::Minted::Placed { id, .. } = minted else {
            panic!("the copy folder owes a placement");
        };
        assert_eq!(id, "b1", "the copy that is there is the row the import names");
        assert_eq!(books.len(), 1, "and no second copy of one file is made");
    }

    // -------------------------------------------------------------------
    // A member of the tree standing outside it.
    // -------------------------------------------------------------------

    /// A shelf cut from a watched folder, hanging on `parent`.
    fn rung(
        id: &str,
        name: &str,
        folder_id: &str,
        rel: Option<&str>,
        parent: Option<&str>,
    ) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: library_core::shelf::ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: rel.map(str::to_string),
            },
            books: Vec::new(),
            parent: parent.map(str::to_string),
            manual_parent: false,
        }
    }

    /// The reported shape: `Root/ > Mid/ > Deep/` imported as one tree, the
    /// `Deep` rung removed, `Deep/` then imported on its own so it stands at the
    /// top level. A re-import of `Root/` finds every book already standing.
    ///
    /// The owner comes back with the state and is held beside it: the signals
    /// are the owner's, so a test that drops it and then reads one is a test
    /// that panics on a disposed value rather than on its own assertion.
    fn displaced_state() -> (AppState, Owner) {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut outer = folder("f1", "/root", &[7], Vec::new());
        outer.shelf_map.insert(String::new(), "s1".to_string());
        outer.shelf_map.insert("mid".to_string(), "s2".to_string());
        let mut inner = folder("f2", "/root/mid/deep", &[7], Vec::new());
        inner.shelf_map.insert(String::new(), "s3".to_string());
        state.library.folders.set(vec![outer, inner]);
        state.library.shelves.set(vec![
            rung("s1", "root", "f1", None, None),
            rung("s2", "mid", "f1", Some("mid"), Some("s1")),
            rung("s3", "deep", "f2", None, None),
        ]);
        state.library.books.set(vec![linked("b1", "/root/mid/deep/dune.md", 7)]);
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "s3") {
                shelf.books.push("b1".to_string());
            }
        });
        (state, owner)
    }

    #[test]
    fn a_subfolder_imported_on_its_own_is_a_member_standing_outside_the_tree() {
        let (state, _owner) = displaced_state();
        let outer = state.library.folder("f1").expect("the tree");
        let walk = vec![
            found_under("/root", "/root/notes.md", 8),
            found_under("/root", "/root/mid/deep/dune.md", 7),
        ];

        let member = displaced_member(state, &outer, &walk).expect("a member outside the tree");
        assert_eq!(member.folder_id, "f2", "the folder that reads the subfolder on its own");
        assert_eq!(member.rel, "mid/deep", "on the rung its directory names in the tree");
        assert_eq!(member.shelf_id, "s3", "and the shelf the note names is that folder's own");
        assert_eq!(member.shelf_name, "deep");
    }

    #[test]
    fn a_member_inside_the_tree_is_not_displaced() {
        let (state, _owner) = displaced_state();
        // The reader carried the shelf in by hand: it hangs under the tree's
        // own rung, so it is where the reader put it and no run asks about it.
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "s3") {
                shelf.parent = Some("s2".to_string());
                shelf.manual_parent = true;
            }
        });
        let outer = state.library.folder("f1").expect("the tree");
        let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
        assert!(
            displaced_member(state, &outer, &walk).is_none(),
            "a shelf inside the tree is not standing outside it"
        );
    }

    #[test]
    fn a_member_the_walk_found_nothing_under_is_not_the_question() {
        let (state, _owner) = displaced_state();
        let outer = state.library.folder("f1").expect("the tree");
        // A walk of the tree's own root shelf only: the member's ground was not
        // part of what this import found, so it is not this import's question.
        let walk = vec![found_under("/root", "/root/notes.md", 8)];
        assert!(displaced_member(state, &outer, &walk).is_none());
    }

    #[test]
    fn a_copying_subfolder_is_not_a_member_of_the_tree() {
        let (state, _owner) = displaced_state();
        // Its copies are the library's own books rather than the tree's rung,
        // so a stored shelf standing at the top level is none of this run's
        // business — the same rule the gate keeps.
        state.library.folders.update(|folders| {
            if let Some(inner) = folders.iter_mut().find(|f| f.id == "f2") {
                inner.opts.in_place = false;
            }
        });
        let outer = state.library.folder("f1").expect("the tree");
        let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
        assert!(displaced_member(state, &outer, &walk).is_none());
    }

    #[test]
    fn the_shallowest_member_answers_because_the_deeper_one_comes_with_it() {
        let (state, _owner) = displaced_state();
        let mut deeper = folder("f3", "/root/mid", &[9], Vec::new());
        deeper.shelf_map.insert(String::new(), "s4".to_string());
        state.library.folders.update(|folders| folders.push(deeper));
        state.library.shelves.update(|shelves| {
            shelves.push(rung("s4", "mid", "f3", None, None));
        });
        let outer = state.library.folder("f1").expect("the tree");
        let walk = vec![
            found_under("/root", "/root/mid/dune.md", 9),
            found_under("/root", "/root/mid/deep/dune.md", 7),
        ];
        let member = displaced_member(state, &outer, &walk).expect("a member");
        assert_eq!(member.folder_id, "f3", "the rung nearest the root answers");
        assert_eq!(member.rel, "mid");
    }

    #[test]
    fn putting_a_member_back_gives_the_tree_its_rung_and_retires_the_folder() {
        let (state, _owner) = displaced_state();
        // The nested folder's own subfolder, which comes back as a rung under
        // the returning one, and a removal it holds for a book the reader
        // deleted there — which must not be resurrected by the tree's next scan.
        state.library.shelves.update(|shelves| {
            shelves.push(rung("s5", "deeper", "f2", Some("deeper"), Some("s3")));
        });
        state.library.folders.update(|folders| {
            if let Some(inner) = folders.iter_mut().find(|f| f.id == "f2") {
                inner.shelf_map.insert("deeper".to_string(), "s5".to_string());
                inner.placed.insert(fp(9));
                inner.ignored.push(stone(9, "/root/mid/deep/deeper/gone.md", false, Some("s5")));
            }
        });

        super::reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

        let shelves = state.library.shelves.get_untracked();
        let back = shelves.iter().find(|s| s.id == "s3").expect("the returning shelf");
        assert_eq!(back.parent.as_deref(), Some("s2"), "hung on the rung its directory names");
        assert!(!back.manual_parent, "and the disk owns that place again");
        assert_eq!(
            back.kind,
            library_core::shelf::ShelfKind::Folder {
                folder_id: "f1".to_string(),
                rel: Some("mid/deep".to_string())
            },
            "owned by the tree, on the tree's own key"
        );
        assert_eq!(
            back.books,
            vec!["b1".to_string()],
            "its books came with it — a shelf is a list of ids and the ids did not move"
        );
        let deeper = shelves.iter().find(|s| s.id == "s5").expect("its own subfolder");
        assert_eq!(
            deeper.kind,
            library_core::shelf::ShelfKind::Folder {
                folder_id: "f1".to_string(),
                rel: Some("mid/deep/deeper".to_string())
            },
            "and the shelf below it took the key below the returning one"
        );
        assert_eq!(deeper.parent.as_deref(), Some("s3"), "still hanging under it");

        let folders = state.library.folders.get_untracked();
        assert_eq!(folders.len(), 1, "one ground, one folder");
        let tree = &folders[0];
        assert_eq!(tree.id, "f1");
        assert_eq!(tree.shelf_map.get("mid/deep").map(String::as_str), Some("s3"));
        assert_eq!(tree.shelf_map.get("mid/deep/deeper").map(String::as_str), Some("s5"));
        assert!(
            tree.placed.contains(&fp(7)) && tree.placed.contains(&fp(9)),
            "the ledger followed the ground, so the tree's next scan adds nothing back"
        );
        assert!(
            tree.ignored.iter().any(|entry| entry.fp == fp(9) && !entry.moved),
            "and the removal the nested folder held is the tree's now"
        );
    }

    #[test]
    fn putting_a_member_back_mints_the_rungs_the_tree_lost() {
        let (state, _owner) = displaced_state();
        // The reader removed the rung ABOVE the member too, so the tree has no
        // shelf for "mid": a returning shelf hung on nothing renders nowhere.
        state.library.shelves.update(|shelves| shelves.retain(|s| s.id != "s2"));
        state.library.folders.update(|folders| {
            if let Some(outer) = folders.iter_mut().find(|f| f.id == "f1") {
                outer.shelf_map.remove("mid");
            }
        });

        super::reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let tree = &folders[0];
        let mid = tree.shelf_map.get("mid").expect("the rung above was minted");
        let mid = shelves.iter().find(|s| &s.id == mid).expect("and stands");
        assert_eq!(mid.name, "mid", "named by its directory");
        assert_eq!(mid.parent.as_deref(), Some("s1"), "hanging on the tree's root shelf");
        let back = shelves.iter().find(|s| s.id == "s3").expect("the returning shelf");
        assert_eq!(back.parent.as_deref(), Some(mid.id.as_str()), "with the member under it");
    }

    #[test]
    fn an_answer_about_a_shelf_that_went_does_nothing_at_all() {
        let (state, _owner) = displaced_state();
        // The note can outlive the shelf it names: a reader who removes it
        // while the modal is up gets no move, and no folder folded away for
        // nothing.
        state.library.shelves.update(|shelves| shelves.retain(|s| s.id != "s3"));

        super::reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

        let folders = state.library.folders.get_untracked();
        assert_eq!(folders.len(), 2, "the nested folder is still the nested folder");
        assert!(folders.iter().any(|f| f.id == "f2"));
        let tree = folders.iter().find(|f| f.id == "f1").expect("the tree");
        assert_eq!(
            tree.shelf_map.get("mid/deep"),
            None,
            "and nothing was folded into it"
        );
    }

    /// The fold is the note's old answer, run instead of offered: the member
    /// standing outside the tree goes back on the rung its directory names,
    /// the folder reading it folds into the tree's ledger, and the run's
    /// report names the shelf that went home — the light the note's close
    /// rides lands on the shelf in its new place rather than on the rung the
    /// tree lost.
    #[test]
    fn the_fold_puts_the_member_back_and_names_the_shelf_it_seated() {
        let (state, _owner) = displaced_state();
        let outer = state.library.folder("f1").expect("the tree");
        let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
        let plan = super::RootPlan::default();

        let folded = super::run_fold(state, &plan, &outer, None, &walk)
            .expect("a member outside the tree is a fold the run owes");
        assert_eq!(folded.0, "s3", "the report names the member's shelf");
        assert_eq!(folded.1, "deep", "speaking the name it wore");

        let shelves = state.library.shelves.get_untracked();
        let back = shelves.iter().find(|s| s.id == "s3").expect("the shelf");
        assert_eq!(
            back.parent.as_deref(),
            Some("s2"),
            "back on the rung its directory names"
        );
        assert!(
            !back.manual_parent,
            "and the disk owns that place again"
        );
        assert_eq!(
            state.library.folders.get_untracked().len(),
            1,
            "one ground, one folder"
        );

        // A second run finds no member outside the tree: the fold already
        // happened, and a run that folds nothing reports nothing.
        let again = super::run_fold(state, &plan, &outer, None, &walk);
        assert!(again.is_none(), "the member is inside the tree now");
    }

    /// The plan's own fold outruns the search for members: a run that walked
    /// the PICKED folder's ledger seats the shelf that run minted, on the
    /// rung the plan named, whatever else stands where.
    #[test]
    fn the_planned_fold_seats_the_run_s_own_shelf() {
        let (state, _owner) = displaced_state();
        let inner = state.library.folder("f2").expect("the picked folder");
        let plan = super::RootPlan {
            fold: Some(("f1".to_string(), "mid/deep".to_string())),
            ..Default::default()
        };
        let folded = super::run_fold(state, &plan, &inner, Some("s3"), &[])
            .expect("the plan names the rung and the run names the shelf");
        assert_eq!(folded.0, "s3");
        let shelves = state.library.shelves.get_untracked();
        let back = shelves.iter().find(|s| s.id == "s3").expect("the shelf");
        assert_eq!(back.parent.as_deref(), Some("s2"));
        assert_eq!(state.library.folders.get_untracked().len(), 1);
    }

    // -------------------------------------------------------------------
    // The read-at-place gate and the copies a run owes.
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

        let mut doomed = replace_rows_of_tree(state, "/books");
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
