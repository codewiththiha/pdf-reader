//! What a drop MEANS, decided from three facts and nothing else: what is held,
//! what is under the pointer, and how long it has been there.
//!
//! Pure on purpose — no DOM, no signals, no state. That is what makes the table
//! testable on the host rather than in a browser, and what makes
//! `crate::features::library::dnd::commit` a dispatch rather than a second set of
//! rules: by the time an effect reaches it, every question a reader could have
//! asked the gesture has already been answered here.
//!
//! The one judgement call in the table is the fold. Resting over a book while
//! holding two or more items offers to make a shelf out of them, and the offer
//! has to be distinguishable from the drop that lands on the same book — which is
//! why the dwell is a question the table is asked rather than a timer the table
//! runs.

use super::target::DropTargetKind;
use crate::features::library::folder_card::THUMB_CAP;

/// How many items must be HELD before the drag offers to fold them.
///
/// Two, because a shelf made from a single book is a shelf the reader could have
/// made from the view menu, and offering one on every drag that rested over a
/// card would be an offer nobody asked for. The book under the pointer is not one
/// of the two: it is what the held items are folded WITH, which is why the plate
/// [`fold_items`] counts lights three cells for a hold of two.
pub const FOLD_MIN_ITEMS: usize = 2;

/// What a release over the hot target would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropEffect {
    /// Put the held books down at this book's place in the level's order.
    InsertBefore { book_id: String },
    /// Take the held items to this shelf. An empty id is the library's root,
    /// which is not a shelf: the books come off whatever held them and the
    /// folders come up to the top level.
    FileToShelf { shelf_id: String },
    /// Put the held items inside this folder.
    NestInto { folder_id: String },
    /// Make a shelf out of the held items and this book, and file them in it.
    CreateFolder { with_book_id: String },
    /// This target refuses what is being held — a folder that would end up
    /// inside itself. Distinct from "nothing under the pointer", which is a
    /// `None` effect rather than this one, so a card can tell "not a target"
    /// from "a target that says no".
    Refused,
}

impl DropEffect {
    /// The book a release would land the held items before, if that is the
    /// effect. What a card asks to decide whether to draw its insertion line —
    /// one accessor rather than a pattern match at every card that wants to know,
    /// because "is this effect mine" is the effect's question to answer.
    pub fn insert_before(&self) -> Option<&str> {
        match self {
            DropEffect::InsertBefore { book_id } => Some(book_id.as_str()),
            _ => None,
        }
    }

    /// The folder a release would file the held items into, if that is the
    /// effect. `None` for a folder that refused the drag, which is the answer a
    /// card that would otherwise promise a drop it cannot honour needs.
    pub fn nest_into(&self) -> Option<&str> {
        match self {
            DropEffect::NestInto { folder_id } => Some(folder_id.as_str()),
            _ => None,
        }
    }
}

/// The plate the drag layer draws while a fold brews: the folder the drop is
/// about to make, filling in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldPreview {
    /// How many of a folder card's four cells the new shelf would fill. Capped
    /// at the plate's own cap, because a preview that showed seven cells would
    /// be a preview of a shape the library does not draw.
    pub filled: usize,
    /// The book being folded with, so the card under the pointer wears the
    /// fold's ring instead of the insertion line.
    pub with_book_id: String,
}

/// Everything the table needs to know, and nothing it could work out for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropQuery<'a> {
    pub held_books: usize,
    pub held_folders: usize,
    pub target_kind: DropTargetKind,
    /// The item the target stands for. Empty for the level's own empty space at
    /// the root of the library, which is how "no shelf" is spelled.
    pub target_id: &'a str,
    /// Whether the target is one of the held items. A book dragged onto itself
    /// folds into a shelf of what is already held rather than counting itself
    /// twice, and a folder dropped on itself is a drop that goes nowhere.
    pub target_is_held: bool,
    /// Whether every held shelf may legally be filed inside the target. Only
    /// asked of a folder target, and only answered by
    /// `library_core::shelf::can_nest`.
    pub can_nest: bool,
    /// Whether the pointer has rested over the target long enough for a fold to
    /// be offered.
    pub dwell_armed: bool,
}

impl DropQuery<'_> {
    /// How many items are being held, of either kind.
    pub fn held(&self) -> usize {
        self.held_books + self.held_folders
    }
}

/// How many items a fold over this target would put in the new shelf: the ones
/// held, plus the book under the pointer unless it is already one of them.
///
/// The plate's count rather than the gate's — a hold of two over a third book is
/// a shelf of three, and the preview has to show three or it is a preview of a
/// different shelf than the one the drop makes.
pub fn fold_items(query: &DropQuery<'_>) -> usize {
    query.held() + usize::from(!query.target_is_held)
}

/// The answer to "what would a release here mean".
pub fn drop_effect(query: DropQuery<'_>) -> DropEffect {
    match query.target_kind {
        DropTargetKind::Book => {
            // The fold outranks the insertion line only once it has been asked
            // for: the same book under the same drag means "put it here" until
            // the dwell says the reader meant "make a shelf of these". Gated on
            // what is HELD rather than on what the new shelf would contain, so a
            // drag of one book that happens to rest over a second stays the
            // reorder it looks like.
            if query.dwell_armed && query.held() >= FOLD_MIN_ITEMS {
                return DropEffect::CreateFolder {
                    with_book_id: query.target_id.to_string(),
                };
            }
            DropEffect::InsertBefore {
                book_id: query.target_id.to_string(),
            }
        }
        DropTargetKind::Folder => {
            // A book is always welcome. A shelf is welcome unless filing it here
            // would put it inside itself, which is a folder no level renders and
            // a reader can never open again.
            if query.held_folders > 0 && !query.can_nest {
                return DropEffect::Refused;
            }
            DropEffect::NestInto {
                folder_id: query.target_id.to_string(),
            }
        }
        // A crumb and the level's own empty space are one answer at two
        // altitudes: the held items go to that level. WHICH level the empty
        // space belongs to is the page's fact rather than the target's, so the
        // controller resolves it before the table is asked.
        DropTargetKind::Shelf | DropTargetKind::Level => DropEffect::FileToShelf {
            shelf_id: query.target_id.to_string(),
        },
    }
}

/// The plate to draw for a fold of `items`, or `None` when it is not a fold.
pub fn fold_preview(items: usize, with_book_id: &str) -> Option<FoldPreview> {
    (items >= FOLD_MIN_ITEMS).then(|| FoldPreview {
        filled: items.min(THUMB_CAP),
        with_book_id: with_book_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A query over `id` holding `books` books and `folders` folders, with no
    /// dwell and nothing refused. The individual tests move the one knob they
    /// are about.
    fn query(kind: DropTargetKind, id: &str, books: usize, folders: usize) -> DropQuery<'_> {
        DropQuery {
            held_books: books,
            held_folders: folders,
            target_kind: kind,
            target_id: id,
            target_is_held: false,
            can_nest: true,
            dwell_armed: false,
        }
    }

    #[test]
    fn a_book_under_a_drag_is_where_the_held_items_land() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Book, "b2", 1, 0)),
            DropEffect::InsertBefore {
                book_id: "b2".to_string()
            }
        );
    }

    #[test]
    fn resting_over_a_book_while_holding_two_makes_a_shelf() {
        let mut held = query(DropTargetKind::Book, "b2", 2, 0);
        // The dwell is the whole of the difference: the same pointer over the
        // same card a moment earlier means "put it here".
        assert!(matches!(drop_effect(held), DropEffect::InsertBefore { .. }));
        held.dwell_armed = true;
        assert_eq!(
            drop_effect(held),
            DropEffect::CreateFolder {
                with_book_id: "b2".to_string()
            }
        );
    }

    #[test]
    fn one_held_item_never_folds_however_long_it_rests() {
        // The gate is on what is held, not on what the new shelf would contain:
        // a drag of one book resting over a second is still the reorder it looks
        // like, and folding there would take a shelf of one away from the reader.
        let mut held = query(DropTargetKind::Book, "b2", 1, 0);
        held.dwell_armed = true;
        assert!(matches!(drop_effect(held), DropEffect::InsertBefore { .. }));
    }

    #[test]
    fn the_plate_counts_the_hovered_book_unless_it_is_already_held() {
        let held = query(DropTargetKind::Book, "b2", 2, 0);
        assert_eq!(fold_items(&held), 3, "two held plus the one rested on");
        let mut same = query(DropTargetKind::Book, "b2", 2, 0);
        same.target_is_held = true;
        assert_eq!(fold_items(&same), 2, "a book already held is not counted twice");
    }

    #[test]
    fn a_mix_of_books_and_shelves_folds_too() {
        let mut held = query(DropTargetKind::Book, "b2", 1, 1);
        held.dwell_armed = true;
        assert_eq!(fold_items(&held), 3);
        assert!(matches!(drop_effect(held), DropEffect::CreateFolder { .. }));
    }

    #[test]
    fn a_shelf_nests_into_a_shelf_unless_it_would_close_a_loop() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 0, 1)),
            DropEffect::NestInto {
                folder_id: "f1".to_string()
            }
        );
        let mut refused = query(DropTargetKind::Folder, "f1", 0, 1);
        refused.can_nest = false;
        assert_eq!(drop_effect(refused), DropEffect::Refused);
    }

    #[test]
    fn a_nesting_is_never_refused_by_what_the_books_are() {
        // `can_nest` is about the shelves being filed, so a drag of books only
        // is welcome in a folder whatever the folder's own ancestry is.
        let mut books_only = query(DropTargetKind::Folder, "f1", 3, 0);
        books_only.can_nest = false;
        assert!(matches!(drop_effect(books_only), DropEffect::NestInto { .. }));
    }

    #[test]
    fn a_crumb_and_the_empty_level_are_the_same_answer() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Shelf, "s1", 1, 1)),
            DropEffect::FileToShelf {
                shelf_id: "s1".to_string()
            }
        );
        assert_eq!(
            drop_effect(query(DropTargetKind::Level, "s1", 1, 1)),
            DropEffect::FileToShelf {
                shelf_id: "s1".to_string()
            }
        );
        // …and the root's empty space spells "no shelf" as an empty id, which
        // is what tells the commit step to take the books off theirs.
        assert_eq!(
            drop_effect(query(DropTargetKind::Level, "", 1, 0)),
            DropEffect::FileToShelf {
                shelf_id: String::new()
            }
        );
    }

    #[test]
    fn the_fold_plate_counts_every_item_and_stops_at_four_cells() {
        // Below the gate there is no plate at all, whatever was counted.
        assert_eq!(fold_preview(1, "b2"), None);
        assert_eq!(
            fold_preview(2, "b2"),
            Some(FoldPreview {
                filled: 2,
                with_book_id: "b2".to_string()
            })
        );
        assert_eq!(fold_preview(3, "b2").unwrap().filled, 3);
        assert_eq!(fold_preview(4, "b2").unwrap().filled, THUMB_CAP);
        assert_eq!(fold_preview(9, "b2").unwrap().filled, THUMB_CAP);
    }
}
