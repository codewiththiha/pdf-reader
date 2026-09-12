//! The read-at-place gate in front of a folder import: the family questions
//! a pick of ground answers BEFORE any sheet or walk — already imported, a
//! continuation of the tree's own root, a fold back into the family — and the
//! [`RootPlan`] the walk then starts from. The stored arrival asks the family
//! nothing and goes straight to the level's own name question.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{self as folder_ops, rel_under, FolderOpts, WatchedFolder};
use library_core::id;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};

use super::claim::{already_importing, claim_root, root_is_claimed};
use super::folder::run_folder;
use super::tasks::{push_task, task_id};
use super::{rel_of, root_shelf_of, shelf_name, Asked};
use crate::services::library::conflict;
use crate::services::library::folder_label;
use crate::state::library::{ImportTask, NoteKind};
use crate::state::AppState;
use crate::time::now_ms;

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
pub(super) fn covered_shelf(state: AppState, root: &str) -> Option<(String, String, String)> {
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
pub(super) fn displaced_member(
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
pub(super) struct DisplacedMember {
    pub(super) folder_id: String,
    pub(super) rel: String,
    pub(super) shelf_id: String,
    pub(super) shelf_name: String,
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
pub(super) fn run_fold(
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
