//! The moves a reader makes by hand: a drag between shelves, a removal, a
//! relink.
//!
//! One rule covers all three, and it is why this module can be short — **an
//! in-app move never touches the filesystem.** A shelf holds book ids, so a drag
//! edits a list of ids; a read-in-place book can be filed anywhere in the app
//! without the file it points at ever being renamed, moved or copied. The only
//! byte this module deletes belongs to a book the app itself copied into its
//! store, and that goes through the shell's contained `delete_stored`.
//!
//! The rule is also what makes the ledger's hardest row true without anybody
//! having to remember it: dragging a book off a watched folder's shelf leaves
//! its fingerprint in that folder's `placed` set, so the next rescan skips it
//! instead of filing it straight back where the reader just moved it from.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Origin};
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf::{self, Shelf, ALL_SHELF, shelf_add};
use library_core::wire::StoreRequest;

use super::covers::prune_now;
use crate::services::library as wire;
use crate::state::{AppState, Toast};

/// Move books: onto `to` at `index`, and off `from` when the two differ.
///
/// A whole drag in one call, whatever it held. A drag of four books is one action
/// from the reader's side, and the blob is written once for it — the rule
/// [`purge_books`] gives for a bulk removal, for the same reason: a reader who
/// closes the window halfway through a move should find all of it or none of it.
///
/// Dropping on the root ([`ALL_SHELF`]) re-orders the library's own list rather
/// than a shelf, because "All" IS that list and not a shelf holding a copy of it.
/// `index` is `None` for "append", which is what a drop on empty space means.
///
/// `index` counts the level the reader pointed at BEFORE the lift, and the two
/// placements below correct for the books the lift shifts left — a correction one
/// book needs once and four books need together, because the second of them lands
/// where the first one just was.
///
/// Only ever called with an index while the view is in its manual order: a shelf
/// sorted by title re-sorts on the next render, so a drop there would be undone
/// before the reader saw it land. `library_core::view::LibraryView::drag_reorders`
/// is the question the drop asks before it names a position.
pub fn move_many_to_shelf(
    state: AppState,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
) {
    if book_ids.is_empty() {
        return;
    }
    if to == ALL_SHELF {
        state
            .library
            .books
            .update(|books| reorder_root(books, book_ids, index));
        crate::storage::persist_library(state.library);
        return;
    }

    state.library.shelves.update(|shelves| {
        if let Some(from) = from.as_deref().filter(|id| *id != to)
            && let Some(shelf) = shelves.iter_mut().find(|s| s.id == from)
        {
            for book_id in book_ids {
                shelf::forget(&mut shelf.books, book_id);
            }
        }
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == to) {
            place_many(&mut shelf.books, book_ids, index);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Take books off a shelf without filing them anywhere else.
///
/// What a drop on the root crumb means from inside a shelf: the reader lifted
/// them OUT, and the root is a level rather than a shelf, so there is no member
/// list to move them to. The books stay in the library — a shelf holds ids and
/// never held a byte — and the folder ledger is untouched, so a watched folder
/// that placed one of them still has its fingerprint and will not offer it back
/// on the next rescan.
pub fn unfile_books(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) else {
            return;
        };
        for book_id in book_ids {
            moved |= shelf::forget(&mut shelf.books, book_id);
        }
    });
    // A drop that changed nothing writes nothing: a reader who puts a book back
    // where it was has not edited the library, and a save is a chance to close
    // the window mid-write.
    if moved {
        crate::storage::persist_library(state.library);
    }
}

/// Re-order the library's own list, which IS the "All" level.
///
/// Lifted out and put back in together rather than one at a time: each book's
/// removal shifts the tail left, so moving four in sequence would have the second
/// one's index mean something the first one's already changed.
fn reorder_root(books: &mut Vec<Book>, book_ids: &[String], index: Option<usize>) {
    let mut lifted: Vec<(usize, Book)> = book_ids
        .iter()
        .filter_map(|book_id| {
            let was = books
                .iter()
                .position(|book| book.id.as_str() == book_id.as_str())?;
            Some((was, books[was].clone()))
        })
        .collect();
    if lifted.is_empty() {
        return;
    }
    // Back to front, so the positions counted above are still true when they are
    // removed.
    let mut positions: Vec<usize> = lifted.iter().map(|(was, _)| *was).collect();
    positions.sort_unstable_by_key(|was| std::cmp::Reverse(*was));
    for was in positions {
        books.remove(was);
    }
    let shift = index.map_or(0, |at| {
        lifted.iter().filter(|(was, _)| *was < at).count()
    });
    // Put back in the order the reader held them, not the order the list did.
    lifted.sort_by_key(|(_, book)| {
        book_ids
            .iter()
            .position(|book_id| book_id.as_str() == book.id.as_str())
            .unwrap_or(usize::MAX)
    });
    insert_many(books, lifted.into_iter().map(|(_, book)| book), index, shift);
}

/// Put `book_ids` on a member list at `index`, taking them off it first.
///
/// [`shelf::place`] for one book and this for a drag: `place` retains and inserts,
/// which is the same two steps, and doing them per book would leave each one's
/// index counting a list the last one had already changed.
fn place_many(members: &mut Vec<String>, book_ids: &[String], index: Option<usize>) {
    let shift = index.map_or(0, |at| {
        book_ids
            .iter()
            .filter(|book_id| {
                members
                    .iter()
                    .position(|member| member.as_str() == book_id.as_str())
                    .is_some_and(|was| was < at)
            })
            .count()
    });
    for book_id in book_ids {
        shelf::forget(members, book_id);
    }
    insert_many(members, book_ids.iter().cloned(), index, shift);
}

/// Insert `items` at `index`, less the `shift` the lift took off the front of it,
/// and in order — each one after the last rather than each one at the same place,
/// which would put them back reversed.
fn insert_many<T>(list: &mut Vec<T>, items: impl Iterator<Item = T>, index: Option<usize>, shift: usize) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
}

/// What a removal is allowed to take with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeOpts {
    /// Delete the app's own copy of a stored book. Only ever offered for a book
    /// the app copied; a linked book's bytes belong to the reader and are never
    /// touched whatever this says.
    pub delete_store_copy: bool,
}

impl Default for PurgeOpts {
    /// On, because a copy the app made for a book that is no longer in the library
    /// is a file nothing will ever read again — and the sheet that offers the
    /// switch is the place to say otherwise.
    fn default() -> Self {
        Self {
            delete_store_copy: true,
        }
    }
}

/// Remove books, and everything the library holds about each of them. One book is
/// a batch of one: the sheet is the only caller and it always holds a list.
///
/// Seven things have to happen together per book, and doing any of them alone
/// leaves something behind that nothing will ever collect: the row (which is the
/// resume point), every shelf membership, the cover, the highlights, the store copy
/// when the app made one, and — in the folders that placed it — a tombstone. The
/// tombstone is the one that is easy to forget and expensive to: without it the file
/// is still on disk and still admitted by the folder's options, so the next focus
/// rescan puts the book straight back. It is per book and per folder for the same
/// reason a batch is not one tombstone: two of a removed ten may have come from
/// different watched folders, and each has to be kept out of its own.
///
/// One persist for the batch rather than one per book. A bulk removal writes the
/// whole blob, and writing it nine times for ten books is nine chances for the
/// reader to close the window mid-way through them.
///
/// Safe while one of the books is open in the reader. `close_document` and the
/// reading-progress debounce both *update* an entry they find and do nothing when
/// they do not, and `shelf::record` only runs on an open — so a purge never
/// resurrects itself from the document it was purged under.
pub fn purge_books(state: AppState, book_ids: &[String], opts: PurgeOpts) {
    let doomed: Vec<Book> = state.library.books.with_untracked(|books| {
        books
            .iter()
            .filter(|b| book_ids.contains(&b.id))
            .cloned()
            .collect()
    });
    if doomed.is_empty() {
        return;
    }
    for book in &doomed {
        purge_one(state, book, opts);
    }

    // The cover cap only holds if an eviction takes its art with it, and the blob
    // is written once for the batch.
    prune_now(state);
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

/// One book's half of a removal. Reads the world before writing any of it, because
/// the tombstone needs the folder that placed this book and the shelf it was filed
/// on, and both are about to change.
fn purge_one(state: AppState, book: &Book, opts: PurgeOpts) {
    let book_id = book.id.as_str();
    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let placed_by = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .map(|f| f.id.clone());
    let home = placed_by
        .as_deref()
        .and_then(|folder_id| folder_shelf_of(&shelves, folder_id, &book.id));
    let entry = Tombstone::of(book, home, js_sys::Date::now() as u64);
    let path = book.path().to_string();
    let stored_copy = match &book.origin {
        Origin::Stored { store, .. } if opts.delete_store_copy => Some(store.clone()),
        _ => None,
    };

    state.library.books.update(|books| {
        library_core::book::remove_book(books, book_id);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, book_id));
    // Only the folders that placed it: removing a book the reader added by hand
    // must not poison a watched folder that happens to hold the same file, and
    // removing a book one folder placed must not stop a second folder from ever
    // offering it.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    state.library.covers.update(|covers| {
        covers.remove(&path);
    });
    // The highlights are the largest thing the library holds about a book besides
    // its cover, and they are keyed by an address nothing points at any more.
    crate::storage::remove_gloss(&path);
    if let Some(store) = stored_copy {
        wire::delete_stored(&store);
    }
}

/// The first of one folder's shelves a book is filed on, in shelf order.
///
/// One answer rather than every answer, because a removed book comes back to ONE
/// shelf and a tombstone that listed three would have to choose at restore time
/// with less information than it has now.
fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| {
            s.kind.folder_id() == Some(folder_id) && s.books.iter().any(|m| m == book_id)
        })
        .map(|s| s.id.clone())
}

/// File a book on a second shelf without moving it.
///
/// One book, two memberships, and nothing copied anywhere — a shelf holds ids, so
/// "also show it here" is the cheapest thing in the app and the one that cannot go
/// wrong on disk. The folder's ledger is untouched too: the book stays placed
/// where it was placed, which is what keeps the next rescan quiet about it.
pub fn also_show(state: AppState, book_id: &str, shelf_id: &str) {
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
            shelf_add(shelf, book_id);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Make a shelf the reader owns, at the level they are looking at, and drill
/// into it. Returns its id.
///
/// Named "New shelf" and left there on purpose: a modal that asks for a name
/// before the shelf exists is a modal the reader has to answer to find out what
/// they were asking for, and the breadcrumb's rename is one keystroke away and
/// shows the shelf it is naming.
pub fn new_shelf(state: AppState) -> String {
    let id = create_shelf(state);
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf the reader owns INSIDE `parent`, and drill into it. Returns its
/// id.
///
/// What a folder's own right-click mints: a shelf made from inside a folder is
/// that folder being subdivided, so the parent is the folder that was asked
/// rather than the level the page happens to be on — a shelf the reader made
/// three folders down appears three folders down, whichever level they are
/// standing on. The drill-in is [`new_shelf`]'s, for the reason it gives.
///
/// No `can_nest` question: a shelf with no children yet closes no loop, and a
/// virtual shelf filed inside a folder shelf is a filing the next rescan leaves
/// alone — the scan re-hangs the folder's own rungs and nothing else.
pub fn new_shelf_in(state: AppState, parent: &str) -> String {
    let id = create_shelf_at(state, Some(parent.to_string()));
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf and stay where you are. What a bulk "file onto a new shelf" wants:
/// the reader picked books on one shelf and asked for them to be on another, and
/// navigating them away from the shelf they were looking at is an answer to a
/// question they did not ask.
///
/// Filed at the level the reader is looking at, because a shelf made from inside a
/// folder is a folder being subdivided and one made from the root is a new top
/// level; "All" is not a shelf, so it is the root.
pub fn create_shelf(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

/// The mint both "new shelf" doors share: one id, one empty virtual row at the
/// level `parent` names, and the search tick that lands it on the frame it is
/// made.
fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(js_sys::Date::now() as u64);
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf {
            id: made,
            name: "New shelf".to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: Vec::new(),
            parent,
        });
    });
    // A belt-and-braces tick for an open search: the folder filter reads the
    // query and the shelves inside one derive, and re-setting the query
    // guarantees both are seen together on the frame the shelf lands — a new
    // shelf under an open search appears at once rather than waiting for the
    // next keystroke to re-run the filter it should already have passed.
    state.library.query.set(state.library.query.get_untracked());
    id
}

/// File one shelf inside another, or back out to the level `parent` names when it
/// is `None`. True when the shelf moved.
///
/// The cycle check is `library_core::shelf::reparent`'s and not the caller's: a
/// folder filed inside itself renders on no level at all and can never be opened
/// again, so the rule has to hold for every caller rather than for every caller
/// that remembered. A refusal writes nothing and persists nothing, which is what
/// lets a drop answer "no" by doing nothing.
pub fn nest_shelf(state: AppState, folder_id: &str, parent: Option<&str>) -> bool {
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        moved = shelf::reparent(shelves, folder_id, parent);
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
    moved
}

/// File several shelves inside one at once. What a bulk "add to shelf" does with
/// the folders in the set: the books are memberships and the folders are nestings,
/// and one persist covers the batch.
pub fn nest_many(state: AppState, folder_ids: &[String], parent: &str) {
    if folder_ids.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        for folder_id in folder_ids {
            // Each one is asked separately: a batch that contained a folder and
            // one of its own children must file the first and refuse the second,
            // and a single all-or-nothing answer would lose one of the two.
            moved |= shelf::reparent(shelves, folder_id, Some(parent));
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
}

/// Move shelves beside one of their own kind: into the anchor's level, at the
/// anchor's place in it, before or after. What a drag onto a shelf ROW's edge
/// commits — the sibling seam the list layout draws — and a reorder rather than
/// a filing wherever the two shelves already share a level, which is the common
/// case: the reader is not changing the tree, they are changing the order the
/// level renders it in.
///
/// Two steps per shelf because the shelf list IS the render order: `reparent`
/// writes the edge (and refuses the loop the graph would close, or a rung the
/// disk owns), and the splice writes the position — `children_of` filters the
/// list in order, so a moved row that kept its old place in the vec would keep
/// its old place on the page. The anchor's index is re-found after every lift,
/// because a removal above it shifts it, and one persist covers the batch.
pub fn reorder_shelves_to_anchor(state: AppState, ids: &[String], anchor: &str, after: bool) {
    if ids.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let parent = shelves
            .iter()
            .find(|s| s.id == anchor)
            .and_then(|s| s.parent.clone());
        for id in ids {
            if id == anchor || !shelf::reparent(shelves, id, parent.as_deref()) {
                continue;
            }
            // Both positions found before the lift: removing the row first
            // would move the anchor under a hand that had already aimed.
            let (Some(at), Some(mut ai)) = (
                shelves.iter().position(|s| s.id == *id),
                shelves.iter().position(|s| s.id == anchor),
            ) else {
                continue;
            };
            let item = shelves.remove(at);
            if at < ai {
                ai -= 1;
            }
            shelves.insert(if after { ai + 1 } else { ai }, item);
            moved = true;
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
}

/// File several books on one shelf at once.
///
/// Membership only, so the same rule covers a bulk filing as covers a drag: a
/// shelf holds ids, nothing here touches a filesystem, and a book already on the
/// shelf is not moved to the end of it for being named twice.
pub fn file_many(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    state.library.shelves.update(|shelves| {
        let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) else {
            return;
        };
        for book_id in book_ids {
            shelf_add(shelf, book_id);
        }
    });
    crate::storage::persist_library(state.library);
}

/// Rename a shelf. A blank name is refused rather than stored: a crumb with
/// nothing on it is a crumb the reader cannot click, and a shelf tile with no name
/// is a strip of covers with no way in.
pub fn rename_shelf(state: AppState, shelf_id: &str, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let name = name.to_string();
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == shelf_id) {
            shelf.name = name;
        }
    });
    crate::storage::persist_library(state.library);
}

/// Take a shelf apart. The books stay in the library — a shelf is a list of ids
/// and never held a byte — and the page steps back out a level, because the thing
/// it was looking at is gone.
///
/// The shelves inside it move up to the level it was on, for the same reason the
/// books stay: a child left pointing at a parent that is gone renders on no level
/// at all, and a reader who removed one folder did not ask to lose the folders
/// filed in it. Stepping out goes to the removed shelf's own parent rather than
/// always to the root, so removing a folder three levels down leaves the reader
/// two levels down and not at the top of the library.
///
/// A folder's shelf is removable too, and the receipt says what that means: the
/// shelf comes off the list and the folder keeps watching, so it returns if the
/// folder ever places a book in it again. That is the honest reading of "watched"
/// rather than a control that appears to work and then does not — the shelf map's
/// pointer is cut here, so a returning shelf is a new shelf, not a ghost.
pub fn delete_shelf(state: AppState, shelf_id: &str) {
    let was_inside = state.library.shelf.get_untracked() == shelf_id;
    let stepped_out = state
        .library
        .shelves
        .with_untracked(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == shelf_id)
                .and_then(|gone| gone.parent.clone())
        })
        .unwrap_or_else(|| ALL_SHELF.to_string());
    // The folder tree's own pointer at this shelf, cut as well. Left in place,
    // a watched folder that places a book here again would file it onto a shelf
    // that no longer exists — a ghost row the reader can neither see nor remove,
    // and the one way a removal could lose a book rather than a shelf.
    let detached = state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .and_then(|s| s.kind.folder_id().map(str::to_string))
    });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
    });
    if let Some(folder_id) = detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folders.iter_mut().find(|f| f.id == folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
            }
        });
    }
    if was_inside {
        state.library.shelf.set(stepped_out);
    }
    crate::storage::persist_library(state.library);
}

/// The shelves a book is on, as `(id, name)` pairs in shelf order. What the
/// folder's restore menu asks in order to tell a book that moved from one that is
/// still where it was filed.
pub fn memberships(state: AppState, book_id: &str) -> Vec<(String, String)> {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.books.iter().any(|m| m == book_id))
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    })
}

/// Re-point a book whose address died at a file the reader picks.
///
/// A linked book takes the new address. A stored book does NOT become linked —
/// that would quietly turn "the app keeps its own copy" back into "the app reads
/// your folder again" — so the pick is copied into the store once more, from
/// wherever the file lives now, and the copy is made BEFORE anything is written:
/// a failure to copy leaves the row exactly as it was.
pub fn relink_book(state: AppState, book_id: String, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        let checks = match wire::verify_paths(vec![path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return toast(state, message),
        };
        let Some(fp) = checks.first().and_then(|c| c.fingerprint()) else {
            return toast(
                state,
                "That file is not there any more. Pick the book's current location.".to_string(),
            );
        };
        let origin = state.library.books.with_untracked(|books| {
            books
                .iter()
                .find(|b| b.id == book_id)
                .map(|b| b.origin.clone())
        });
        let Some(origin) = origin else {
            return;
        };

        let store = match origin {
            Origin::Linked { .. } => None,
            Origin::Stored { .. } => {
                let task = format!("relink-{book_id}");
                let requests = [StoreRequest {
                    path: path.clone(),
                    id: book_id.clone(),
                }];
                match wire::store_books(&task, &requests).await {
                    Ok(results) => match results.into_iter().next() {
                        Some(result) if result.is_ok() => Some(result.store),
                        Some(result) => {
                            let message = result
                                .error
                                .unwrap_or_else(|| "Could not copy that file.".to_string());
                            return toast(state, message);
                        }
                        None => return toast(state, "Could not copy that file.".to_string()),
                    },
                    Err(message) => return toast(state, message),
                }
            }
        };

        state.library.books.update(|books| {
            let Some(book) = books.iter_mut().find(|b| b.id == book_id) else {
                return;
            };
            match &mut book.origin {
                Origin::Linked { src } => *src = path.clone(),
                Origin::Stored { src, store: at } => {
                    *src = Some(path.clone());
                    if let Some(store) = store.as_ref() {
                        *at = store.clone();
                    }
                }
            }
            book.fp = fp;
            book.fp_pending = false;
            book.missing = false;
        });
        // Covers are keyed by address, so the old entry now belongs to nobody;
        // the prune drops it and the next open renders the new one.
        prune_now(state);
        crate::storage::persist_library(state.library);
        crate::storage::persist_covers(state.library);
        // The old address's cover belongs to nobody now, and the new one has
        // never been rendered: queue it rather than waiting for an open.
        super::covers::backfill_missing(state);
    });
}

/// Ask the reader for a file and relink to it.
///
/// The picker is the engine's own (`pdf_engine::api::pick_document`) rather than
/// a second dialog implementation here: it is the same question — "which
/// document?" — with the same filter, and a cancel is the same non-event.
pub fn relink_dialog(state: AppState, book_id: String) {
    spawn_local(async move {
        match pdf_engine::api::pick_document().await {
            Ok(path) => relink_book(state, book_id, path),
            // A cancel is the reader changing their mind, not a failure.
            Err(message) if message == "Open cancelled" => {}
            Err(message) => toast(state, message),
        }
    });
}

fn toast(state: AppState, message: String) {
    state.ui.toast.set(Some(Toast::new(message)));
}
