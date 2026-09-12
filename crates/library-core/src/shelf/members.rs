//! Shelf membership: the ids a shelf holds, the level query that answers
//! for the root, and the edits a placement, a filing and a removal make.

use super::{Shelf, ALL_SHELF};

/// Put `id` on a member list at `index`, or move it there when it is already a
/// member. `None` appends. The whole of the drag-and-drop contract: one
/// ordered list, one id, one index.
///
/// Moving within a list removes first and then inserts, so dropping a book on
/// its own neighbour does not shift the tail — the index the reader pointed at
/// is the index the book lands at.
pub fn place(members: &mut Vec<String>, id: &str, index: Option<usize>) {
    members.retain(|m| m != id);
    let at = index.unwrap_or(members.len()).min(members.len());
    members.insert(at, id.to_string());
}

/// The ids of the rows one level holds, in the order it holds them.
///
/// The one answer to "what is on this level", which every rule that used to
/// spell it out twice — a collision check and a count, a render and a purge —
/// reads instead. A shelf answers with its member list; the root answers with
/// the rows no shelf holds, because the root IS a level and this is its list.
/// A shelf id that names no shelf and is not the root answers with nothing.
pub fn members_of<'a>(
    rows: &'a [crate::book::Row],
    shelves: &'a [Shelf],
    shelf_id: &str,
) -> Vec<&'a str> {
    if let Some(shelf) = shelves.iter().find(|s| s.id == shelf_id) {
        return shelf.books.iter().map(String::as_str).collect();
    }
    if shelf_id != ALL_SHELF {
        return Vec::new();
    }
    // One pass over the memberships rather than one per row: a library at its
    // cap holds two thousand rows, and this is asked per arrival and per
    // render.
    let filed: std::collections::HashSet<&str> = shelves
        .iter()
        .flat_map(|s| s.books.iter().map(String::as_str))
        .collect();
    rows.iter()
        .map(crate::book::Row::id)
        .filter(|id| !filed.contains(id))
        .collect()
}

/// Put `id` on a shelf, unless it is already on it.
///
/// Not [`place`]: a restore and a "show it here as well" both add a book that may
/// already be a member, and appending it again would move a book the reader can
/// see to the end of a shelf for no reason. `place` is for a drag, which is an
/// instruction about position; this is for a filing, which is not.
pub fn shelf_add(shelf: &mut Shelf, book_id: &str) {
    if !shelf.books.iter().any(|member| member == book_id) {
        shelf.books.push(book_id.to_string());
    }
}

/// Take `id` off a member list. True when it was there.
pub fn forget(members: &mut Vec<String>, id: &str) -> bool {
    let before = members.len();
    members.retain(|m| m != id);
    members.len() != before
}

/// Drop a book from every shelf at once — what removing a book from the
/// library does. Membership is per shelf and a book can be on several, so the
/// sweep is the only way to be sure no shelf keeps pointing at a row that is
/// gone.
pub fn forget_everywhere(shelves: &mut [Shelf], book_id: &str) {
    for shelf in shelves.iter_mut() {
        forget(&mut shelf.books, book_id);
    }
}

/// The shelves a book is on. What a remove-from-shelf context row lists, and
/// what tells a drag's source shelf from its target.
pub fn containing<'a>(shelves: &'a [Shelf], book_id: &str) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.books.iter().any(|m| m == book_id))
        .collect()
}
