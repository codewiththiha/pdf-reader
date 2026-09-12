//! The shelf tree: which shelves hang under which, and the moves that
//! re-hang them — the cycle-checked nest, the subtree a departing shelf
//! takes with it, and the pass that puts a watched folder's rungs back on
//! the seats their directories name.

use super::{find, Shelf, ShelfKind};

/// The shelves filed directly inside `parent_id`, in the order the library
/// stores them. `None` asks for the root level, which is what the page shows
/// while it is drilled out of every shelf.
///
/// Direct children only: a level is a page, and a view that flattened the whole
/// subtree would be showing the reader shelves they have not opened.
pub fn children_of<'a>(shelves: &'a [Shelf], parent_id: Option<&str>) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.parent.as_deref() == parent_id)
        .collect()
}

/// The chain above `id`, root first and excluding `id` itself — what a
/// breadcrumb walks to draw the way back out.
///
/// The walk stops on a shelf it has already seen. [`sanitize`] makes a cycle
/// unreachable in a loaded blob, but this answers a signal that can be read
/// between two writes, and a breadcrumb that looped would hang the render
/// rather than show one crumb too many.
pub fn ancestors<'a>(shelves: &'a [Shelf], id: &str) -> Vec<&'a Shelf> {
    let mut chain: Vec<&'a Shelf> = Vec::new();
    let mut next = find(shelves, id).and_then(|s| s.parent.as_deref());
    while let Some(parent_id) = next {
        if chain.iter().any(|seen| seen.id == parent_id) {
            break;
        }
        let Some(parent) = shelves.iter().find(|s| s.id == parent_id) else {
            break;
        };
        chain.push(parent);
        next = parent.parent.as_deref();
    }
    chain.reverse();
    chain
}

/// Whether `folder_id` may be filed inside `target_id`.
///
/// Two refusals, and both are about the same failure: a shelf inside itself is
/// not a shelf the reader can reach. `folder_id == target_id` is the drop on
/// itself; the walk up from the target is the drop into one of its own
/// descendants, which is the same cycle one level down.
pub fn can_nest(shelves: &[Shelf], folder_id: &str, target_id: &str) -> bool {
    if folder_id == target_id {
        return false;
    }
    let mut current = Some(target_id.to_string());
    // Bounded by the list rather than by the walk finding its own tail: a blob
    // that already carries a cycle would otherwise spin here forever, and the
    // honest answer about a broken graph is "no".
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return true;
        };
        if id == folder_id {
            return false;
        }
        current = shelves
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.parent.clone());
    }
    false
}

/// File `folder_id` inside `parent`, or at the root when `parent` is `None`.
/// True when the shelf was found and the graph allows the move.
///
/// Refuses the move [`can_nest`] refuses, and leaves the list exactly as it
/// was: a drop that would close a cycle is a drop that never happened, which
/// is what lets the caller answer a refusal by doing nothing at all.
///
/// Every shelf may be moved, including one cut from a watched tree — and for
/// a folder shelf the mark is an honest COMPARISON rather than a blanket:
/// [`Shelf::manual_parent`] records that the shelf hangs off the seat its
/// folder's own shelves name for it (`folder_seat`). A hand that takes a
/// rung off its seat marks it, and the next re-hang passes it by; a hand that
/// puts one BACK on its seat clears the mark, and the disk owns the place
/// again; a re-order on the seat the shelf already hangs on writes the same
/// answer it had. The mark is written HERE, in the one function every
/// hand-move rides (a drag's nest, a bulk filing, a sibling reorder), rather
/// than at call sites that would each have to remember it.
///
/// The read-at-place rung a hand drags off its seat does not arrive here at
/// all: it departs as a copy first ([`departs_on_move`]), and what rides this
/// function afterwards is the virtual shelf the copy became.
pub fn reparent(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    if let Some(target) = parent
        && !can_nest(shelves, folder_id, target)
    {
        return false;
    }
    // The seat is read before the write borrow: which place the disk names is
    // a fact about the list as it stands, not about the shelf mid-move.
    let seat = folder_seat(shelves, folder_id);
    let Some(shelf) = shelves.iter_mut().find(|s| s.id == folder_id) else {
        return false;
    };
    // A watched shelf keeps every fact it has except its place: the `rel`
    // that routes its folder's new files into it travels with the move, and
    // the mark below is what stops the next re-hang from undoing a hand the
    // disk disagrees with — and nothing else.
    if let Some(seat) = seat {
        shelf.manual_parent = seat.as_deref() != parent;
    }
    shelf.parent = parent.map(str::to_string);
    true
}

/// The folder's rungs: every shelf of one folder, from the `rel` key to the
/// shelf id wearing it. One map for the seat question and the re-hang,
/// because both ask which shelf a rung key names and the two cannot drift.
fn rungs_of<'a>(
    shelves: &'a [Shelf],
    folder_id: &str,
) -> std::collections::HashMap<String, &'a str> {
    shelves
        .iter()
        .filter_map(|s| match &s.kind {
            ShelfKind::Folder {
                folder_id: owner,
                rel,
            } if owner == folder_id => Some((rel.clone().unwrap_or_default(), s.id.as_str())),
            _ => None,
        })
        .collect()
}

/// The parent the folder's own SHELVES name for a folder shelf: the rung
/// above its `rel`, or the library's root for a top-level rung. `None` for a
/// shelf that is no folder's rung — a virtual shelf is the reader's wherever
/// it hangs, and there is no disk answer to compare against.
///
/// The seat the re-hang wants and the mark [`reparent`] writes are one
/// arithmetic: a rung whose parent rung is not standing — removed, or never
/// minted yet — seats at the root, which is the honest answer for a broken
/// tree and the same one [`rehang_moves`] gives.
fn folder_seat(shelves: &[Shelf], shelf_id: &str) -> Option<Option<String>> {
    let shelf = find(shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let key = rel.clone().unwrap_or_default();
    let rungs = rungs_of(shelves, folder_id);
    Some(
        crate::folder::parent_key(&key)
            .and_then(|rung| rungs.get(rung))
            .map(|id| id.to_string()),
    )
}


/// Every shelf below any of `roots`, at any depth, in no particular order,
/// without repeats and without the roots themselves.
///
/// Walked with an explicit stack and a seen-set rather than recursively, for
/// two reasons. The forest is finite because [`sanitize`] cuts cycles out of a
/// loaded blob — but this reads a list that can be caught between two writes,
/// and a recursion over a graph with a loop in it is a stack that never
/// unwinds. A shelf inside itself is also a shelf that would otherwise be
/// counted twice, and two roots can share a descendant, which one question
/// asks once — a removal's cascade and a departure's ride both count what
/// actually goes.
pub fn subtree_ids(shelves: &[Shelf], roots: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = roots.to_vec();
    while let Some(parent) = stack.pop() {
        for child in children_of(shelves, Some(parent.as_str())) {
            if roots.iter().any(|each| each == &child.id)
                || out.iter().any(|each| each == &child.id)
            {
                continue;
            }
            stack.push(child.id.clone());
            out.push(child.id.clone());
        }
    }
    out
}

/// Move a shelf's children up to the level it was on. What taking a shelf apart
/// owes the shelves inside it: a child left pointing at a parent that is gone
/// renders on no level at all, and the reader who removed one folder did not ask
/// to lose the ones filed in it.
pub fn lift_children(shelves: &mut [Shelf], folder_id: &str) {
    let inherited = shelves
        .iter()
        .find(|s| s.id == folder_id)
        .and_then(|s| s.parent.clone());
    for shelf in shelves.iter_mut() {
        if shelf.parent.as_deref() == Some(folder_id) {
            shelf.parent = inherited.clone();
        }
    }
}

/// The moves a watched folder's rescan owes its own shelves: every shelf the
/// folder owns that is not hand-moved, when the rung its `rel` names resolves
/// to a different parent than the one it hangs on.
///
/// The tree on disk is the tree on the shelf, for the shelves this folder
/// owns: a folder card cut from a watched tree is a VIEW of that tree, so its
/// rung is the one its `rel` names — including for shelves an older, flatter
/// build minted as siblings, which this pass re-hangs under the rung they were
/// always cut from. Virtual shelves are the reader's own arrangement and are
/// never touched here, and neither is a shelf of another folder — nor a shelf
/// the reader moved BY HAND, which [`reparent`] marked [`Shelf::manual_parent`]:
/// the hand beats the disk, and this pass is the disk's. A moved shelf still
/// serves as its subfolders' rung in the answer below, so the subtree the
/// reader carried off re-hangs together, wherever it now hangs.
///
/// Answers the moves rather than writing them, so the caller holds one list of
/// `(shelf id, wanted parent)` it can apply in one pass — and so the rule is a
/// pure function a test can hold to account. A shelf is never its own parent,
/// whatever a stale `shelf_map` claims: a self-edge the walk resolved through
/// is filtered here rather than trusted to the sanitizer to catch later.
pub fn rehang_moves(shelves: &[Shelf], folder_id: &str) -> Vec<(String, Option<String>)> {
    // The folder's rungs: its own `rel` to the shelf that carries it — the
    // one map the seat question reads too, so the re-hang and the hand's
    // mark cannot disagree about which shelf a rung names.
    let rungs = rungs_of(shelves, folder_id);
    let mut moved = Vec::new();
    for shelf in shelves.iter() {
        // The reader's placement wins over the disk's shape.
        if shelf.manual_parent {
            continue;
        }
        let ShelfKind::Folder {
            folder_id: owner,
            rel,
        } = &shelf.kind
        else {
            continue;
        };
        if owner != folder_id {
            continue;
        }
        let key = rel.clone().unwrap_or_default();
        let want = crate::folder::parent_key(&key)
            .and_then(|rung| rungs.get(rung).copied())
            // A shelf is never its own parent, whatever a stale map claims.
            .filter(|w| *w != shelf.id.as_str())
            .map(str::to_string);
        if shelf.parent != want {
            moved.push((shelf.id.clone(), want));
        }
    }
    moved
}
