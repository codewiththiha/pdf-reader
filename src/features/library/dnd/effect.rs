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

/// How many items the new shelf must hold before the drag offers to make it.
///
/// Two, because a shelf of one is a shelf the view menu already makes and a drag
/// has nothing to add to it. The book under the pointer is the second one, so the
/// offer starts at the first item held: one book dragged onto another and rested
/// there is a shelf of the two, which is the whole of what folding a pair means.
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
/// held, plus the book under the pointer.
///
/// Counted once, and by construction rather than by a check here — [`drop_effect`]
/// refuses to fold over a book the pointer is already carrying, so the partner is
/// never also one of the held. That is the bug this used to have: a target that
/// counted itself made a drag of two onto one of the two look like a shelf of
/// three, and a drag of one onto itself look like a shelf of two.
///
/// The plate's count and the gate's are the same number, which is the point of it
/// being one function: a hold of two over a third book is a shelf of three, and a
/// preview that showed two would be a preview of a different shelf than the one the
/// drop makes.
pub fn fold_items(query: &DropQuery<'_>) -> usize {
    query.held() + 1
}

/// The answer to "what would a release here mean".
pub fn drop_effect(query: DropQuery<'_>) -> DropEffect {
    match query.target_kind {
        DropTargetKind::Book => {
            // A book the pointer is already carrying is a POSITION and never a
            // partner. Folding there would count it twice, and "put it here" is
            // what a reader who drags onto their own selection means — including
            // the single book dragged onto itself, which is a reorder that lands
            // where it started and so is the no-op it looks like.
            if query.target_is_held {
                return DropEffect::InsertBefore {
                    book_id: query.target_id.to_string(),
                };
            }
            // Any other book is a partner, but only once the pointer has rested.
            // The dwell is the whole of the difference between the two answers and
            // it is not a refinement: without it a reorder would be unreachable,
            // because every card a drag crossed would be offering a new shelf
            // instead of a place to land. Membership of the payload decides WHICH
            // book could be a partner; the rest decides WHETHER the reader meant
            // one.
            if query.dwell_armed && fold_items(&query) >= FOLD_MIN_ITEMS {
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
        // The ellipsis stands for levels, and is not one. Opening the fold is the
        // controller's answer to a rest here; the table's answer to a RELEASE is
        // that there is nothing to do, which is the honest one — the reader
        // cannot see which level they would be filing onto.
        DropTargetKind::Ellipsis => DropEffect::Refused,
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

    /// The same query with the pointer rested, which is the state a fold needs.
    fn rested(kind: DropTargetKind, id: &str, books: usize, folders: usize) -> DropQuery<'_> {
        DropQuery {
            dwell_armed: true,
            ..query(kind, id, books, folders)
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
    fn resting_over_an_unheld_book_folds_it_with_the_hold() {
        // The dwell is the whole of the difference: the same pointer over the
        // same card a moment earlier means "put it here".
        assert!(matches!(
            drop_effect(query(DropTargetKind::Book, "b2", 1, 0)),
            DropEffect::InsertBefore { .. }
        ));
        // One held book and the one rested on is a shelf of two, which is the
        // smallest shelf a drag can make and the reason the pair gesture exists.
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 1, 0)),
            DropEffect::CreateFolder {
                with_book_id: "b2".to_string()
            }
        );
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 3, 1)),
            DropEffect::CreateFolder {
                with_book_id: "b2".to_string()
            }
        );
    }

    #[test]
    fn a_book_the_pointer_is_carrying_is_a_position_and_never_a_partner() {
        // The bug this rule exists for: a target that counted itself made a drag
        // of one book onto itself a shelf of two, and a multi-select dragged onto
        // one of its own members a shelf that held that member twice.
        let onto_itself = DropQuery {
            target_is_held: true,
            ..rested(DropTargetKind::Book, "b1", 1, 0)
        };
        assert_eq!(
            drop_effect(onto_itself),
            DropEffect::InsertBefore {
                book_id: "b1".to_string()
            }
        );
        let onto_the_set = DropQuery {
            target_is_held: true,
            ..rested(DropTargetKind::Book, "b2", 3, 1)
        };
        assert!(matches!(
            drop_effect(onto_the_set),
            DropEffect::InsertBefore { .. }
        ));
        // And unrested, a held target is the same answer: the dwell cannot rescue
        // a partner that is already in the payload.
        let unrested = DropQuery {
            target_is_held: true,
            ..query(DropTargetKind::Book, "b2", 3, 0)
        };
        assert!(matches!(drop_effect(unrested), DropEffect::InsertBefore { .. }));
    }

    #[test]
    fn the_plate_counts_the_partner_once_and_stops_at_four_cells() {
        // Held plus the one book rested on, never the book twice.
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 1, 0)), 2);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 2, 0)), 3);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 0, 1)), 2);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 3, 1)), 5);

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

    #[test]
    fn a_shelf_nests_into_a_shelf_unless_it_would_close_a_loop() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 0, 1)),
            DropEffect::NestInto {
                folder_id: "f1".to_string()
            }
        );
        let refused = DropQuery {
            can_nest: false,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(refused), DropEffect::Refused);
    }

    #[test]
    fn a_nesting_is_never_refused_by_what_the_books_are() {
        // `can_nest` is about the shelves being filed, so a drag of books only
        // is welcome in a folder whatever the folder's own ancestry is.
        let books_only = DropQuery {
            can_nest: false,
            ..query(DropTargetKind::Folder, "f1", 3, 0)
        };
        assert!(matches!(drop_effect(books_only), DropEffect::NestInto { .. }));
    }

    #[test]
    fn folders_and_crumbs_never_brew_a_shelf_however_long_the_rest() {
        // A fold effect over a folder would be a nest and a create at once, and
        // over a crumb a filing and a create at once: two answers to one release.
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Folder, "f1", 2, 1)),
            DropEffect::NestInto { .. }
        ));
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Shelf, "s1", 2, 0)),
            DropEffect::FileToShelf { .. }
        ));
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Level, "s1", 2, 0)),
            DropEffect::FileToShelf { .. }
        ));
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
    fn the_ellipsis_opens_and_never_accepts() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Ellipsis, "", 2, 1)),
            DropEffect::Refused
        );
        // …and no rest changes that: a fold is made out of a BOOK the pointer is
        // resting on, and the ellipsis is not a book.
        assert_eq!(
            drop_effect(rested(DropTargetKind::Ellipsis, "", 2, 1)),
            DropEffect::Refused
        );
    }
}
