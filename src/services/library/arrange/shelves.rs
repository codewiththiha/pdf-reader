//! The reader's own shelves: made, named, nested, reordered and taken apart.
//! A shelf holds ids and never held a byte, so every operation here is a
//! membership edit — with the departure's question asked of the moves that
//! carry a read-at-place shelf off its folder's seat
//! ([`super::shelf_departure`]).

use leptos::prelude::*;

use library_core::folder::self as folder_ops;
use library_core::shelf::{self as shelf, Shelf, ALL_SHELF};

use crate::state::AppState;
use crate::time::now_ms;

use library_core::id;

use super::shelf_departure::{raise_departure, screen_shelf_moves, SeamSide, ShelfSeam};

/// Make a shelf the reader owns, and drill into it. Returns its id.
///
/// `parent` is where the shelf hangs: `None` is the level the page is on, and
/// `Some` is a shelf the reader named — what a folder's own right-click mints,
/// because a shelf made from inside a folder is that folder being subdivided,
/// so the parent is the folder that was asked rather than the level the page
/// happens to be on. A shelf the reader made three folders down appears three
/// folders down, whichever level they are standing on.
///
/// Named "New shelf" and left there on purpose: a modal that asks for a name
/// before the shelf exists is a modal the reader has to answer to find out what
/// they were asking for, and the breadcrumb's rename is one keystroke away and
/// shows the shelf it is naming.
///
/// No `can_nest` question: a shelf with no children yet closes no loop, and a
/// virtual shelf filed inside a folder shelf is a filing the next rescan leaves
/// alone — the scan re-hangs the folder's own rungs and nothing else.
pub fn create_shelf_and_enter(state: AppState, parent: Option<&str>) -> String {
    let id = match parent {
        Some(parent) => create_shelf_at(state, Some(parent.to_string())),
        None => create_shelf_here(state),
    };
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// Make a shelf at the level the reader is looking at, and stay where you are.
/// What a bulk "file onto a new shelf" wants: the reader picked books on one
/// shelf and asked for them to be on another, and navigating them away from
/// the shelf they were looking at is an answer to a question they did not ask.
///
/// Filed at the level the reader is looking at, because a shelf made from inside a
/// folder is a folder being subdivided and one made from the root is a new top
/// level; "All" is not a shelf, so it is the root.
pub fn create_shelf_here(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

/// The mint both "new shelf" doors share: one id, one empty virtual row at the
/// level `parent` names, and the search tick that lands it on the frame it is
/// made.
fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(now_ms());
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf {
            id: made,
            name: "New shelf".to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: Vec::new(),
            parent,
            manual_parent: false,
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

// ---------------------------------------------------------------------------
// The shelf's departure: a read-at-place shelf becomes the library's own copy.
// ---------------------------------------------------------------------------

/// File one shelf inside another, or back out to the level `parent` names when it
/// is `None`. True when the shelf moved.
///
/// The cycle check is `library_core::shelf::reparent`'s and not the caller's: a
/// folder filed inside itself renders on no level at all and can never be opened
/// again, so the rule has to hold for every caller rather than for every caller
/// that remembered. A refusal writes nothing and persists nothing, which is what
/// lets a drop answer "no" by doing nothing.
///
/// Nesting asks no NAME question, and that is the rule rather than an
/// oversight: the question a collision asks is about a name on a level, and a
/// nesting writes no membership — the folder keeps its own member list and
/// hangs inside the parent. It does ask the departure's question, because that
/// one is not about names at all: a read-at-place shelf leaving the seat its
/// folder's tree names for it becomes the library's own copy
/// (`library_core::shelf::departs_on_move`), and the copies a departure makes
/// are a cost the reader is told about before they are made — with the way
/// home beside the cost when the drop landed inside the shelf's family,
/// because a move inside the tree a read-at-place shelf belongs to never has
/// to buy a copy ([`answer_departure_return`]). The clean half of a batch
/// lands now; the departing half goes to the sheet, and its landing is
/// [`confirm_departure`]'s re-dispatch of this very function over the copies.
pub fn nest_shelf(state: AppState, folder_id: &str, parent: Option<&str>) -> bool {
    let one = [folder_id.to_string()];
    let (clean, departing) = screen_shelf_moves(state, &one, parent);
    if !departing.is_empty() {
        raise_departure(state, departing, parent.map(str::to_string), None);
        return false;
    }
    if clean.is_empty() {
        return false;
    }
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
///
/// The folders that actually moved are the ones the persist covers. No NAME
/// question is asked about any of them: a nesting writes no membership, so
/// nothing arrives on the parent's level for a name to collide with (see
/// [`nest_shelf`]). The departure's question IS asked, and it is one sheet for
/// the batch rather than one per shelf: a reader who selected three rungs and
/// filed them made one gesture, and "these three become copies" is the one
/// sentence that answers it. The clean half — the reader's own shelves in the
/// set — lands before the sheet rises, and a cancel leaves it landed.
pub fn nest_many(state: AppState, folder_ids: &[String], parent: &str) {
    if folder_ids.is_empty() {
        return;
    }
    let (clean, departing) = screen_shelf_moves(state, folder_ids, Some(parent));
    if !departing.is_empty() {
        raise_departure(state, departing, Some(parent.to_string()), None);
    }
    if clean.is_empty() {
        return;
    }
    let mut moved_ids: Vec<String> = Vec::new();
    state.library.shelves.update(|shelves| {
        for folder_id in &clean {
            // Each one is asked separately: a batch that contained a folder and
            // one of its own children must file the first and refuse the second,
            // and a single all-or-nothing answer would lose one of the two.
            if shelf::reparent(shelves, folder_id, Some(parent)) {
                moved_ids.push(folder_id.clone());
            }
        }
    });
    if !moved_ids.is_empty() {
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
/// Screened like every other hand-move: a reorder among siblings is the same
/// parent and departs nothing, but a seam in ANOTHER level takes a read-at-place
/// rung off the seat its folder's tree names, and that half goes to the
/// departure's sheet ([`nest_shelf`] gives the rule) while the clean half lands
/// now. The confirm's re-dispatch rides this very function over the copies.
///
/// Two steps per shelf because the shelf list IS the render order: `reparent`
/// writes the edge (and refuses the loop the graph would close), and the splice
/// writes the position — `children_of` filters the list in order, so a moved row
/// that kept its old place in the vec would keep its old place on the page. The
/// anchor's index is re-found after every lift, because a removal above it
/// shifts it, and one persist covers the batch.
pub fn reorder_shelves_to_anchor(state: AppState, ids: &[String], anchor: &str, side: SeamSide) {
    if ids.is_empty() {
        return;
    }
    // The level the seam is in, read once for the screen: the anchor's own
    // parent, which is the level the reorder re-parents into.
    let parent = state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, anchor).and_then(|s| s.parent.clone())
    });
    let (clean, departing) = screen_shelf_moves(state, ids, parent.as_deref());
    if !departing.is_empty() {
        raise_departure(
            state,
            departing,
            parent,
            Some(ShelfSeam {
                anchor_id: anchor.to_string(),
                side,
            }),
        );
    }
    if clean.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let parent = shelf::find(shelves, anchor).and_then(|s| s.parent.clone());
        for id in &clean {
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
            let at = match side {
                SeamSide::After => ai + 1,
                SeamSide::Before => ai,
            };
            shelves.insert(at, item);
            moved = true;
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
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
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
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
    // One read of the shelf list answers both facts about the shelf that is
    // going: the level to step out to, and — the folder tree's own pointer at
    // this shelf, cut as well — which watched folder filed onto it. Left in
    // place, a folder that places a book here again would file it onto a shelf
    // that no longer exists: a ghost row the reader can neither see nor
    // remove, and the one way a removal could lose a book rather than a shelf.
    let (stepped_out, detached) =
        state
            .library
            .shelves
            .with_untracked(|shelves| {
                shelf::find(shelves, shelf_id)
                    .map_or((ALL_SHELF.to_string(), None), |gone| {
                        (
                            gone.parent
                                .clone()
                                .unwrap_or_else(|| ALL_SHELF.to_string()),
                            gone.kind.folder_id().map(str::to_string),
                        )
                    })
            });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
    });
    if let Some(folder_id) = detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folder_ops::find_mut(folders, &folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
            }
        });
    }
    // A link may point AT a shelf — the pointer a merged import leaves behind —
    // and a pointer at nothing is a row that renders, is clicked and does
    // nothing: the shelf links go with the shelf, the way the book links go
    // with a book.
    let shelves_now = state.library.shelves.get_untracked();
    state.library.books.update(|rows| {
        library_core::book::drop_dead_shelf_links(rows, &shelves_now);
    });
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
        shelf::containing(shelves, book_id)
            .into_iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    })
}
