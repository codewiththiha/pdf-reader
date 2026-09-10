//! Shelves: an ordered list of book ids, and the two kinds that produce one.
//!
//! A shelf holds membership and nothing else — no copies, no paths, no
//! filesystem intent. That is what makes dragging a book between shelves safe
//! by construction: the only thing a drop can change is an ordered list of
//! ids, so a read-in-place book can be filed anywhere in the app without the
//! file it points at ever being touched.
//!
//! "All" is deliberately not a shelf. Every book is in it by definition, so
//! storing it would be a second copy of the book list that has to be kept in
//! sync forever; [`ALL_SHELF`] is the id the UI uses for that pseudo-shelf and
//! [`find`] answers `None` for it, which is how a caller tells the two
//! apart.
//!
//! ## Nesting
//!
//! [`Shelf::parent`] makes the shelves a forest rather than a list: the root
//! level is the shelves whose parent is `None`, and a level inside a shelf is
//! [`children_of`] on that shelf's id. Nesting is the one relationship in this
//! module that can be wrong in a way no single row shows — a shelf filed inside
//! itself, or inside one of its own children, is a folder that renders nowhere
//! and can never be opened again. So the graph is guarded twice: [`can_nest`]
//! refuses the drop before it is written, and [`sanitize`] cuts any cycle a
//! hand-edited blob carries, because a rule only enforced on the way in is a
//! rule one restored backup can break.
//!
//! Nesting is NOT the same thing as [`ShelfKind::Folder`]'s `rel`. `rel` is a
//! subfolder's address inside a watched directory's tree — a rescan key, owned
//! by the filesystem. For a folder shelf `parent` starts as its projection —
//! the tree on disk is the tree on the shelf, and a scan re-hangs the folder's
//! shelves on the rungs their `rel` names — until the reader moves one by hand.
//! A hand beats the disk: [`reparent`] accepts the move and marks the row
//! [`Shelf::manual_parent`], which is what makes the next rescan leave it
//! where the reader put it. The shelf keeps its disk knowledge through the
//! move — its `rel` still routes newly scanned files into it — and for a
//! virtual shelf `parent` is the reader's from the start, and no scan ever
//! writes it.

use serde::{Deserialize, Serialize};

/// The pseudo-shelf holding every book: the library's root, and the order the
/// persisted book list is in. Not a [`Shelf`] — see the module docs.
pub const ALL_SHELF: &str = "all";

/// What produced a shelf, which decides whether the UI offers it a folder
/// glyph, a watch dot, or a rename.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShelfKind {
    /// Created by the reader (or by an import of loose files). Pure
    /// membership: nothing on disk corresponds to it. The default, because a
    /// shelf a blob does not describe is one the reader made.
    #[default]
    Virtual,
    /// Cut from a watched folder's tree. `rel` is the subfolder within
    /// [`crate::folder::WatchedFolder::root`], `None` for the root itself —
    /// which is what a rescan matches a found file's
    /// [`crate::scan::FoundFile::subfolder`] against.
    Folder {
        #[serde(rename = "folderId")]
        folder_id: String,
        #[serde(default)]
        rel: Option<String>,
    },
}

impl ShelfKind {
    /// The watched folder this shelf belongs to, if it belongs to one.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            ShelfKind::Virtual => None,
            ShelfKind::Folder { folder_id, .. } => Some(folder_id),
        }
    }

    /// True for the shelf a watched folder's root files onto.
    pub fn is_folder_root(&self) -> bool {
        matches!(self, ShelfKind::Folder { rel: None, .. })
    }
}

/// One shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ShelfKind,
    /// Member book ids, in the order the reader arranged them. The order is
    /// the point: a shelf is a list, not a set, and a drop writes an index.
    #[serde(default)]
    pub books: Vec<String>,
    /// The shelf this one is filed inside, or `None` at the library's root.
    ///
    /// `#[serde(default)]` because a blob written before shelves could nest has
    /// no `parent` key at all, and every shelf in it is a root shelf — which is
    /// the right reading of it rather than a migration.
    #[serde(default)]
    pub parent: Option<String>,
    /// The reader moved this shelf by hand, so its place in the library is the
    /// reader's and not the disk's: a watched folder's rescan re-hangs the
    /// shelves it owns on the rungs their `rel` names, and passes a shelf
    /// wearing this mark by. Written by [`reparent`] and by nothing else;
    /// `#[serde(default)]` because a blob from before shelves could be
    /// hand-moved has no key and every shelf in it is the scan's.
    ///
    /// The shelf keeps everything else it knows: its `rel` is still the rescan
    /// key, its folder's `shelf_map` still routes newly scanned files into it,
    /// and its books keep their read-in-place addresses — a moved shelf moves
    /// its whole subtree, the way a moved directory takes its tree with it.
    #[serde(default)]
    pub manual_parent: bool,
}

impl Shelf {
    /// True when this shelf was cut from a watched folder.
    pub fn is_folder(&self) -> bool {
        self.kind.folder_id().is_some()
    }
}

/// The shelf with this id, if there is one. `None` for [`ALL_SHELF`]: the
/// caller falls back to the whole book list.
pub fn find<'a>(shelves: &'a [Shelf], id: &str) -> Option<&'a Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter().find(|s| s.id == id)
}

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
/// Every shelf may be moved, including one cut from a watched tree — the
/// reader's hand beats the disk's shape — and moving a watched one marks it
/// [`Shelf::manual_parent`], which is what tells the next rescan's re-hang to
/// pass it by instead of putting it back where the directory had it. The mark
/// is written HERE, in the one function every hand-move rides (a drag's nest,
/// a bulk filing, a sibling reorder), rather than at call sites that would
/// each have to remember it.
pub fn reparent(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    if let Some(target) = parent
        && !can_nest(shelves, folder_id, target)
    {
        return false;
    }
    let Some(shelf) = shelves.iter_mut().find(|s| s.id == folder_id) else {
        return false;
    };
    // A watched shelf keeps every fact it has except its rung: the `rel` that
    // routes its folder's new files into it travels with the move, and the
    // mark below is what stops the next re-hang from undoing the hand.
    if shelf.is_folder() {
        shelf.manual_parent = true;
    }
    shelf.parent = parent.map(str::to_string);
    true
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

/// The BOOK rows of one level, resolved: what a count, a content check and the
/// cover queue all want, and never the links — a link has no fingerprint to
/// check, no address to render art from and no page to count.
///
/// A member naming no row is skipped rather than rendered as a hole, which is
/// [`crate::sort::ordered`]'s rule too.
pub fn books_of<'a>(
    rows: &'a [crate::book::Row],
    shelves: &[Shelf],
    shelf_id: &str,
) -> Vec<&'a crate::book::Book> {
    members_of(rows, shelves, shelf_id)
        .into_iter()
        .filter_map(|id| crate::book::find_row(rows, id))
        .filter_map(crate::book::Row::book)
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

/// Make a persisted shelf list internally valid: drop shelves with no id or no
/// name, drop a row wearing the [`ALL_SHELF`] id (the pseudo-shelf is the book
/// list and no level renders it), dedupe by id (first wins), drop members that
/// are blank, drop a duplicate member keeping its first position, and cut the
/// nesting graph back to a forest. Idempotent. Members that name a book the
/// library no longer has are NOT dropped here — that needs the book list, and
/// [`blob::sanitize`](crate::blob::sanitize) does it with both in hand.
pub fn sanitize(shelves: &mut Vec<Shelf>) {
    let mut seen = std::collections::HashSet::new();
    shelves.retain(|s| {
        !s.id.trim().is_empty() && !s.name.trim().is_empty() && s.id != ALL_SHELF
    });
    shelves.retain(|s| seen.insert(s.id.clone()));
    for s in shelves.iter_mut() {
        let mut members = std::collections::HashSet::new();
        s.books.retain(|m| !m.trim().is_empty() && members.insert(m.clone()));
    }

    // A parent that is the shelf itself, or that names no shelf, is a folder no
    // level renders: the row survives the load and vanishes from the page. Both
    // collapse to the root, which is where the reader can see it again.
    for s in shelves.iter_mut() {
        if s.parent.as_deref() == Some(s.id.as_str()) {
            s.parent = None;
        }
    }
    // Owned ids rather than borrowed ones: the pass below writes to the same list
    // it is reading the names out of, and a set of `&str` into it would hold the
    // borrow open across the write.
    let ids: std::collections::HashSet<String> =
        shelves.iter().map(|s| s.id.clone()).collect();
    for s in shelves.iter_mut() {
        if s.parent.as_deref().is_some_and(|p| !ids.contains(p)) {
            s.parent = None;
        }
    }

    // Then the cycles, which no single row shows. Only the shelves ON a loop are
    // cut: a shelf that merely leads into one keeps its parent, and becomes a
    // root shelf's child once the loop below it is open. Cutting on a walk that
    // fails to come back instead would empty a whole branch for one bad edge,
    // and would not be idempotent — the second pass would find nothing to cut.
    let mut on_a_cycle: Vec<String> = Vec::new();
    for s in shelves.iter() {
        let mut current = s.parent.clone();
        for _ in 0..=shelves.len() {
            let Some(parent_id) = current else {
                break;
            };
            if parent_id == s.id {
                on_a_cycle.push(s.id.clone());
                break;
            }
            current = shelves
                .iter()
                .find(|p| p.id == parent_id)
                .and_then(|p| p.parent.clone());
        }
    }
    for s in shelves.iter_mut() {
        if on_a_cycle.contains(&s.id) {
            s.parent = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::book::{Book, Fingerprint, Origin, Row};
    use reader_core::format::Format;

    fn book(id: &str, title: &str) -> Row {
        let mut book = Book::new(
            id.to_string(),
            Fingerprint { size: 1, mtime_ms: 1, head_hash: 1 },
            Format::Pdf,
            Origin::Linked { src: format!("/books/{id}.pdf") },
            1,
        );
        book.title = Some(title.to_string());
        Row::Book(book)
    }

    fn plain(id: &str, members: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Virtual,
            books: members.iter().map(|m| m.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    #[test]
    fn a_level_s_members_are_its_shelf_s_or_the_unfiled_rows() {
        let rows = vec![
            book("b1", "Dune"),
            book("b2", "Apple"),
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
        ];
        let shelves = vec![plain("s", &["b1", "l1"]), plain("t", &["b2"])];
        assert_eq!(members_of(&rows, &shelves, "s"), vec!["b1", "l1"]);
        assert_eq!(members_of(&rows, &shelves, "gone"), Vec::<&str>::new());
        // The root is a level with no shelf row, and its list is what is left.
        assert_eq!(members_of(&rows, &shelves, ALL_SHELF), Vec::<&str>::new());
        let one_filed = vec![plain("s", &["b1"])];
        assert_eq!(members_of(&rows, &one_filed, ALL_SHELF), vec!["b2", "l1"]);
        // Its BOOKS are those members that are books: the link is on the shelf
        // and is not one of its books.
        assert_eq!(books_of(&rows, &shelves, "s").len(), 1);
        assert_eq!(books_of(&rows, &shelves, "s")[0].title(), "Dune");
        assert_eq!(books_of(&rows, &one_filed, ALL_SHELF).len(), 1);
        assert!(books_of(&rows, &shelves, "gone").is_empty());
    }

    use super::*;

    fn shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    /// One shelf filed inside another: the shape a nest produces.
    fn nested(id: &str, name: &str, parent: &str) -> Shelf {
        Shelf {
            parent: Some(parent.to_string()),
            ..shelf(id, name, &[])
        }
    }

    fn ids_of<'a>(shelves: &[&'a Shelf]) -> Vec<&'a str> {
        shelves.iter().map(|s| s.id.as_str()).collect()
    }

    fn ids(members: &[String]) -> Vec<&str> {
        members.iter().map(String::as_str).collect()
    }

    #[test]
    fn all_is_not_a_shelf_and_never_looks_like_one() {
        let shelves = vec![shelf("s1", "Sci-fi", &["a"])];
        assert!(find(&shelves, ALL_SHELF).is_none());
        assert!(find(&shelves, "s1").is_some());
        assert!(find(&shelves, "nope").is_none());
    }

    #[test]
    fn a_pseudo_all_shelf_never_survives_a_load() {
        // A blob that somehow carries an "all" row must not turn into a
        // bookshelf tile duplicating the whole library: the sanitizer drops
        // it, so no level ever renders it and no view has to remember to.
        let mut shelves = vec![shelf(ALL_SHELF, "All", &["a"]), shelf("s1", "Sci-fi", &["a"])];
        sanitize(&mut shelves);
        let ids: Vec<&str> = shelves.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s1"]);
    }

    #[test]
    fn a_drop_appends_by_default() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        place(&mut m, "c", None);
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_drop_lands_at_the_index_pointed_at() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "d", Some(1));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c"]);
        // Past the end clamps rather than panics: a drop on the gap after the
        // last card is an append.
        place(&mut m, "e", Some(99));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c", "e"]);
    }

    #[test]
    fn moving_a_member_does_not_duplicate_or_shift_the_tail() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "c", Some(0));
        assert_eq!(ids(&m), vec!["c", "a", "b"]);
        place(&mut m, "a", Some(1));
        assert_eq!(ids(&m), vec!["c", "a", "b"], "a book stays where it is dropped");
        place(&mut m, "c", Some(3));
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn filing_a_book_that_is_already_filed_moves_nothing() {
        // A restore and an "also show it here" both add a book that may already
        // be a member; appending it again would reshuffle a shelf the reader can
        // see, for an instruction that was not about position.
        let mut s = shelf("s1", "One", &["a", "b"]);
        shelf_add(&mut s, "b");
        assert_eq!(ids(&s.books), vec!["a", "b"]);
        shelf_add(&mut s, "c");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
        shelf_add(&mut s, "a");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_book_leaves_one_shelf_or_all_of_them() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        assert!(forget(&mut m, "a"));
        assert!(!forget(&mut m, "a"));
        assert_eq!(ids(&m), vec!["b"]);

        let mut shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        forget_everywhere(&mut shelves, "b");
        assert_eq!(ids(&shelves[0].books), vec!["a"]);
        assert!(shelves[1].books.is_empty());
    }

    #[test]
    fn the_shelves_a_book_is_on_are_found_in_order() {
        let shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        let names: Vec<&str> = containing(&shelves, "b").iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["One", "Two"]);
        assert!(containing(&shelves, "zzz").is_empty());
    }

    #[test]
    fn a_folder_shelf_knows_its_folder_and_its_subfolder() {
        let root = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: None,
            },
            ..shelf("s1", "Books", &[])
        };
        let sub = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &[])
        };
        assert!(root.is_folder() && sub.is_folder());
        assert!(root.kind.is_folder_root() && !sub.kind.is_folder_root());
        assert_eq!(sub.kind.folder_id(), Some("f1"));
        assert_eq!(shelf("s3", "Mine", &[]).kind.folder_id(), None);
    }

    #[test]
    fn a_folder_kind_persists_with_its_folder_id() {
        let s = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &["a"])
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"folder\""), "{json}");
        assert!(json.contains("\"folderId\":\"f1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // A blob from before `rel` existed is the folder's root shelf.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(older.kind.is_folder_root());
    }

    #[test]
    fn sanitize_dedupes_shelves_and_their_members() {
        let mut shelves = vec![
            shelf("s1", "One", &["a", "a", "b", "  "]),
            shelf("s1", "One again", &["c"]),
            shelf("  ", "Nameless id", &[]),
            shelf("s3", "   ", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves.len(), 1);
        assert_eq!(ids(&shelves[0].books), vec!["a", "b"]);
    }

    #[test]
    fn a_shelf_without_a_kind_still_loads() {
        let s: Shelf = serde_json::from_str(r#"{"id":"s1","name":"One"}"#).unwrap();
        assert_eq!(s.kind, ShelfKind::Virtual);
        assert!(s.books.is_empty());
        assert_eq!(s.parent, None, "a blob from before nesting has no parent");
    }

    #[test]
    fn a_level_is_the_shelves_filed_directly_inside_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Crime", "s1"),
            nested("s4", "Space", "s2"),
        ];
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2", "s3"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s2"))), vec!["s4"]);
        assert!(children_of(&shelves, Some("nope")).is_empty());
    }

    #[test]
    fn the_way_out_of_a_shelf_is_the_chain_above_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
        ];
        assert!(ancestors(&shelves, "s1").is_empty());
        assert_eq!(ids_of(&ancestors(&shelves, "s2")), vec!["s1"]);
        assert_eq!(ids_of(&ancestors(&shelves, "s3")), vec!["s1", "s2"]);
        // "All" is not a shelf, so it has no chain either.
        assert!(ancestors(&shelves, ALL_SHELF).is_empty());
        assert!(ancestors(&shelves, "nope").is_empty());
    }

    #[test]
    fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
        // s3 is inside s2 and both sit at the root, so s1 is the one shelf that is
        // nobody's ancestor: it may go anywhere, and the other two may not go down
        // their own branch.
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(can_nest(&shelves, "s1", "s3"), "s1 is above nothing, so it can go deepest");
        assert!(can_nest(&shelves, "s2", "s1"));
        assert!(can_nest(&shelves, "s3", "s1"), "and a shelf may be lifted out of its branch");
        assert!(!can_nest(&shelves, "s2", "s2"), "a shelf is not inside itself");
        assert!(!can_nest(&shelves, "s1", "s1"));
        assert!(!can_nest(&shelves, "s2", "s3"), "s3 is already inside s2");
        assert!(!can_nest(&shelves, "s2", "s2"));
        // A shelf that is not in the list is not a cycle, so the graph rule
        // allows it; `reparent` is the half that refuses a shelf it cannot find.
        assert!(can_nest(&shelves, "s9", "s1"));
        let mut none: Vec<Shelf> = Vec::new();
        assert!(!reparent(&mut none, "s9", Some("s1")));
    }

    #[test]
    fn a_nest_writes_one_parent_and_a_refusal_writes_nothing() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2"]);
        // s2 now holds s3, so filing s1 inside s3 closes the loop and is refused
        // with the list untouched.
        assert!(!can_nest(&shelves, "s1", "s3"));
        assert!(!reparent(&mut shelves, "s1", Some("s3")));
        assert_eq!(shelves[0].parent, None);
        // Back out to the root.
        assert!(reparent(&mut shelves, "s2", None));
        assert_eq!(shelves[1].parent, None);
    }

    #[test]
    fn a_hand_moved_watched_shelf_keeps_its_move_and_says_so() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            Shelf {
                kind: ShelfKind::Folder {
                    folder_id: "f1".into(),
                    rel: Some("scifi".into()),
                },
                ..shelf("s2", "Watched", &[])
            },
        ];
        // The reader's hand beats the disk's shape: the move lands, and the row
        // carries the mark the next re-hang reads — which is what makes the
        // move a promise the rescan KEEPS instead of one it breaks.
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert!(shelves[1].manual_parent);
        // Its disk knowledge is the move's survivor: the kind — and with it
        // the `rel` that routes the folder's new files into this very shelf —
        // is untouched by a change of place.
        assert!(matches!(
            shelves[1].kind,
            ShelfKind::Folder { ref rel, .. } if rel.as_deref() == Some("scifi")
        ));
        // A virtual shelf's move is the reader's from the start: no scan ever
        // wrote its parent, so there is no scan to tell to stand aside.
        assert!(reparent(&mut shelves, "s1", None));
        assert!(!shelves[0].manual_parent);
        // And the loop rule holds for a watched shelf exactly as for any
        // other: s2 is inside s1, so s1 inside s2 is refused, mark or no mark.
        assert!(!reparent(&mut shelves, "s1", Some("s2")));
        assert_eq!(shelves[0].parent, None);
    }

    #[test]
    fn a_shelf_from_before_the_mark_existed_is_the_scans() {
        // No `manualParent` key in an older blob is not a hand-move: every
        // shelf in it is where the last scan put it, and the next one may
        // re-hang it.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(!older.manual_parent);
        // And a shelf the reader moved persists as one.
        let moved = Shelf {
            manual_parent: true,
            parent: Some("s1".into()),
            ..older.clone()
        };
        let json = serde_json::to_string(&moved).unwrap();
        assert!(json.contains("\"manualParent\":true"), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, moved);
    }

    #[test]
    fn taking_a_shelf_apart_lifts_the_shelves_inside_it() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        lift_children(&mut shelves, "s2");
        assert_eq!(
            shelves[2].parent.as_deref(),
            Some("s1"),
            "s3 inherits the level s2 was on, not the top of the library"
        );
        assert_eq!(shelves[3].parent, None, "a shelf elsewhere is not touched");
        lift_children(&mut shelves, "s1");
        assert_eq!(shelves[1].parent, None, "a root shelf's children become roots");
        assert_eq!(
            shelves[2].parent, None,
            "including the one that just moved up into it"
        );
    }

    #[test]
    fn sanitize_collapses_a_parent_that_names_no_shelf() {
        let mut shelves = vec![nested("s1", "Orphan", "gone"), nested("s2", "Filed", "s1")];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None, "an orphan is a root, not a hole");
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"), "its child is untouched");
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn sanitize_opens_a_cycle_and_leaves_the_branch_above_it_alone() {
        // s1 is filed in s2, and s2 and s3 are filed in each other: the loop is
        // the pair, and s1 only leads into it.
        let mut shelves = vec![
            nested("s1", "Leads in", "s2"),
            nested("s2", "Loop a", "s3"),
            nested("s3", "Loop b", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves[1].parent, None, "both edges of the loop are cut");
        assert_eq!(shelves[2].parent, None);
        assert_eq!(
            shelves[0].parent.as_deref(),
            Some("s2"),
            "a shelf that leads into the loop keeps the parent it had"
        );
        assert_eq!(shelves[3].parent, None);
        // Nothing is unreachable: every shelf is on some level again.
        let mut on_a_level: Vec<&str> = [None, Some("s1"), Some("s2"), Some("s3")]
            .iter()
            .flat_map(|at| children_of(&shelves, *at))
            .map(|s| s.id.as_str())
            .collect();
        on_a_level.sort_unstable();
        assert_eq!(on_a_level, vec!["s1", "s2", "s3", "s4"]);
        // Idempotent: a second pass finds a forest and changes nothing.
        let before = shelves.clone();
        sanitize(&mut shelves);
        assert_eq!(shelves, before);
    }

    #[test]
    fn a_shelf_filed_inside_itself_is_put_back_at_the_root() {
        let mut shelves = vec![Shelf {
            parent: Some("s1".into()),
            ..shelf("s1", "Self", &[])
        }];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None);
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn a_parent_persists_with_the_shelf() {
        let s = nested("s2", "Sci-fi", "s1");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"parent\":\"s1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
