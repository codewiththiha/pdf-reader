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

/// The shelves whose id is not [`ALL_SHELF`], in the order the library stores
/// them — the order the grid renders bookshelves in.
pub fn real(shelves: &[Shelf]) -> Vec<&Shelf> {
    shelves.iter().filter(|s| s.id != ALL_SHELF).collect()
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
/// name, dedupe by id (first wins), drop members that are blank, and drop a
/// duplicate member keeping its first position. Idempotent. Members that name
/// a book the library no longer has are NOT dropped here — that needs the book
/// list, and [`blob::sanitize`](crate::blob::sanitize) does it with both in
/// hand.
pub fn sanitize(shelves: &mut Vec<Shelf>) {
    let mut seen = std::collections::HashSet::new();
    shelves.retain(|s| !s.id.trim().is_empty() && !s.name.trim().is_empty());
    shelves.retain(|s| seen.insert(s.id.clone()));
    for s in shelves.iter_mut() {
        let mut members = std::collections::HashSet::new();
        s.books.retain(|m| !m.trim().is_empty() && members.insert(m.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
        }
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
        assert_eq!(real(&shelves).len(), 1);
    }

    #[test]
    fn a_pseudo_all_shelf_is_never_rendered_as_a_bookshelf() {
        // A blob that somehow carries an "all" row must not turn into a
        // bookshelf tile duplicating the whole library.
        let shelves = vec![shelf(ALL_SHELF, "All", &["a"]), shelf("s1", "Sci-fi", &["a"])];
        assert_eq!(real(&shelves).len(), 1);
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
    }
}
