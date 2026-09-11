//! What a drop MEANS, decided from four facts and nothing else: what is held,
//! what is under the pointer, WHICH PART of the target's box the pointer is on,
//! and how long it has been there.
//!
//! Pure on purpose — no DOM, no signals, no state. That is what makes the table
//! testable on the host rather than in a browser, and what makes
//! `crate::features::library::dnd::commit` a dispatch rather than a second set of
//! rules: by the time an effect reaches it, every question a reader could have
//! asked the gesture has already been answered here.
//!
//! The part-of-the-box fact is the list's: a row is thin, so its edges are seams
//! rather than a single "here". The bottom half of a book row lands the hold
//! AFTER its anchor, the middle half of a shelf row files the hold INSIDE it,
//! and a shelf row's outer quarters reorder the held folders beside it in the
//! level that holds them — the three cues a file manager's tree gives a drag.
//! The grid does not ask the question: outside the list layout every band is the
//! middle one, and a card keeps the whole-card answer it has always had.
//!
//! The one judgement call in the table is the fold, and it is BOOK over book:
//! resting a hold with books in it over an unheld book offers to make a shelf
//! out of them, and the offer has to be distinguishable from the drop that
//! lands on the same book — which is why the dwell is a question the table is
//! asked rather than a timer the table runs. A hold with no books in it is
//! refused by a book row outright: a row is a seam between books, a shelf is
//! not a book, and a folder over a book is a stacking nobody asked for —
//! folders land in a folder's mouth, beside their own kind on a shelf row's
//! edges, on a crumb or on the level's own space.

use super::target::DropTargetKind;

/// The most covers a plate — and so a fold preview — fills.
const THUMB_CAP: usize = library_core::view::PLATE_CELLS;

/// How many items the new shelf must hold before the drag offers to make it.
///
/// Two, because a shelf of one is a shelf the view menu already makes and a drag
/// has nothing to add to it. The book under the pointer is the second one, so the
/// offer starts at the first item held: one book dragged onto another and rested
/// there is a shelf of the two, which is the whole of what folding a pair means.
pub const FOLD_MIN_ITEMS: usize = 2;

/// Which part of a target's box the pointer is on.
///
/// The list's question — a row is thin and its edges are seams — and the grid's
/// non-question: the session answers `Middle` for every target outside the list
/// layout, which is how a card keeps its whole-card semantics without the table
/// carrying a branch about layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Top,
    Middle,
    Bottom,
}

impl Band {
    /// Whether this band lands the hold on the far side of its anchor: the
    /// bottom edge is "after", every other edge is "before".
    pub fn after(self) -> bool {
        self == Band::Bottom
    }
}

/// What a release over the hot target would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropEffect {
    /// Put the held books down at this book's place in the order of the shelf
    /// that renders its row — before the anchor, or after it when the seam was
    /// the row's bottom edge.
    InsertBefore {
        book_id: String,
        /// The shelf whose member list renders the anchor row, or `None` for
        /// the library's own order at the root. Named by the effect rather
        /// than re-derived at the commit: the row that was hit is the row that
        /// knows its container, and a nested tree row's is not the open level.
        shelf: Option<String>,
        after: bool,
    },
    /// Reorder the held folders beside this shelf, inside the level that holds
    /// it. Edges of a shelf row only, and folders only: a mixed hold on a shelf
    /// row is a hold going INSIDE it, and a book has no siblings among shelves.
    ShelfSibling { anchor_id: String, after: bool },
    /// Take the held items to this shelf. An empty id is the library's root,
    /// which is not a shelf: the books come off whatever held them and the
    /// folders come up to the top level.
    FileToShelf { shelf_id: String },
    /// Put the held items inside this folder.
    NestInto { folder_id: String },
    /// Make a shelf out of the held items and this book, and file them in it.
    CreateFolder { with_book_id: String },
    /// This target refuses what is being held — a folder that would end up
    /// inside itself, a sibling the graph says no to, or a book row asked
    /// about a hold with no books in it. Distinct from "nothing under the
    /// pointer", which is a `None` effect rather than this one, so a card can
    /// tell "not a target" from "a target that says no".
    Refused,
}

impl DropEffect {
    /// The book a release would land the held items beside, and on which side
    /// of it, if that is the effect. What a row asks to decide which of its two
    /// seams to draw — one accessor rather than a pattern match at every row
    /// that wants to know, because "is this effect mine" is the effect's
    /// question to answer.
    pub fn insert_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::InsertBefore {
                book_id, after, ..
            } => Some((book_id.as_str(), *after)),
            _ => None,
        }
    }

    /// The shelf a release would reorder the held folders beside, and on which
    /// side of it, if that is the effect.
    pub fn sibling_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::ShelfSibling { anchor_id, after } => Some((anchor_id.as_str(), *after)),
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
    /// Whether every held shelf may sit beside the target as a sibling — the
    /// PARENT's `can_nest` answer, because filing beside a shelf is filing into
    /// the level that holds it. Only asked of a folder target; the edges of its
    /// row are the only place a sibling is a question.
    pub can_sibling: bool,
    /// Which part of the target's box the pointer is on. The list's seams;
    /// `Middle` everywhere else. See [`Band`].
    pub band: Band,
    /// The shelf whose member list renders the target row, resolved by the
    /// session: the entry's own when the row named one, else the open level,
    /// and `None` at the root. What an insertion names as its container.
    pub target_shelf: Option<&'a str>,
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
            let id = query.target_id;
            let shelf = query.target_shelf.map(str::to_string);
            // A book the pointer is already carrying is a POSITION and never a
            // partner. Folding there would count it twice, and "put it here" is
            // what a reader who drags onto their own selection means — including
            // the single book dragged onto itself, which is a reorder that lands
            // where it started and so is the no-op it looks like.
            if query.target_is_held {
                return DropEffect::InsertBefore {
                    book_id: id.to_string(),
                    shelf,
                    after: false,
                };
            }
            // A hold with no books in it has nothing to say to a book row: a
            // row is a seam BETWEEN books and a shelf is not a book, so there
            // is no position to take and no fold to brew — the fold is book
            // over book. Folders land in a folder's mouth, beside their own
            // kind on a shelf row's edges, on a crumb or on the level's space,
            // and a refusal here is what keeps the plate and the ring off a
            // rest that would promise a shelf nobody asked for.
            if query.held_books == 0 {
                return DropEffect::Refused;
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
                    with_book_id: id.to_string(),
                };
            }
            DropEffect::InsertBefore {
                book_id: id.to_string(),
                shelf,
                after: query.band.after(),
            }
        }
        DropTargetKind::Folder => {
            let id = query.target_id;
            // The middle of a folder is its mouth: a book is always welcome,
            // and a shelf is welcome unless filing it here would put it inside
            // itself, which is a folder no level renders and a reader can never
            // open again.
            if query.band == Band::Middle {
                if query.held_folders > 0 && !query.can_nest {
                    return DropEffect::Refused;
                }
                return DropEffect::NestInto {
                    folder_id: id.to_string(),
                };
            }
            // The edges are seams between SHELVES — but only for a hold that is
            // shelves alone. Books on an edge, or a mixed hold, still go inside:
            // a book has no position among a level's folders, and splitting one
            // hold two ways at one release is two answers to one question.
            if query.held_books > 0 || query.held_folders == 0 {
                if query.held_folders > 0 && !query.can_nest {
                    return DropEffect::Refused;
                }
                return DropEffect::NestInto {
                    folder_id: id.to_string(),
                };
            }
            // Folders alone on an edge: a sibling reorder, refused by the same
            // graph the nest is — the parent's `can_nest` — and by the drop on
            // itself, which has no seam to speak of.
            if query.target_is_held || !query.can_sibling {
                return DropEffect::Refused;
            }
            DropEffect::ShelfSibling {
                anchor_id: id.to_string(),
                after: query.band.after(),
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
            can_sibling: false,
            band: Band::Middle,
            target_shelf: None,
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

    /// The book's answer with the position written out.
    fn insert(id: &str, shelf: Option<&str>, after: bool) -> DropEffect {
        DropEffect::InsertBefore {
            book_id: id.to_string(),
            shelf: shelf.map(str::to_string),
            after,
        }
    }

    #[test]
    fn a_book_under_a_drag_is_where_the_held_items_land() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Book, "b2", 1, 0)),
            insert("b2", None, false)
        );
    }

    #[test]
    fn the_bottom_half_of_a_book_row_lands_the_hold_after_it() {
        // The list's seam: one row, two positions, and the band is the whole
        // of the difference between them.
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Bottom,
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", None, true)
        );
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Top,
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", None, false),
            "the top half is the before it has always been"
        );
        // …and the row's own shelf rides along, so a nested row lands in the
        // shelf that renders it rather than in the level the page is on.
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Bottom,
                target_shelf: Some("s2"),
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", Some("s2"), true)
        );
    }

    #[test]
    fn a_hold_with_no_books_in_it_is_refused_by_a_book_row() {
        // A row is a seam between books and a shelf is not a book: no band of
        // the row, no container behind it and no rest changes that answer.
        assert_eq!(
            drop_effect(query(DropTargetKind::Book, "b2", 0, 1)),
            DropEffect::Refused
        );
        assert_eq!(
            drop_effect(DropQuery {
                target_shelf: Some("s2"),
                ..query(DropTargetKind::Book, "b2", 0, 2)
            }),
            DropEffect::Refused
        );
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 0, 1)),
            DropEffect::Refused,
            "a dwell offers a fold to a hold with books in it, and nothing to this"
        );
        // A mixed hold keeps the books' position; the folders ride the same
        // container at the commit rather than splitting the release in two.
        assert_eq!(
            drop_effect(DropQuery {
                target_shelf: Some("s2"),
                band: Band::Bottom,
                ..query(DropTargetKind::Book, "b2", 1, 1)
            }),
            insert("b2", Some("s2"), true)
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
        // A mixed hold folds too: the new shelf takes the books AND the
        // folders the reader was carrying, plus the partner.
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
        assert_eq!(drop_effect(onto_itself), insert("b1", None, false));
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
    fn the_middle_of_a_shelf_row_takes_the_hold_inside() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 0, 1)),
            DropEffect::NestInto {
                folder_id: "f1".to_string()
            }
        );
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 2, 0)),
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
    fn the_edges_of_a_shelf_row_reorder_siblings() {
        // Folders alone on an edge are a seam between SHELVES: the drop files
        // them into the anchor's own level, before or after the anchor.
        let sibling = |band: Band| DropQuery {
            band,
            can_sibling: true,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(
            drop_effect(sibling(Band::Top)),
            DropEffect::ShelfSibling {
                anchor_id: "f1".to_string(),
                after: false
            }
        );
        assert_eq!(
            drop_effect(sibling(Band::Bottom)),
            DropEffect::ShelfSibling {
                anchor_id: "f1".to_string(),
                after: true
            }
        );
    }

    #[test]
    fn books_on_a_shelf_rows_edge_still_go_inside_it() {
        // A book has no position among a level's folders, and a mixed hold is
        // one question: both go in, the way the middle answers.
        for band in [Band::Top, Band::Bottom] {
            assert_eq!(
                drop_effect(DropQuery {
                    band,
                    can_sibling: true,
                    ..query(DropTargetKind::Folder, "f1", 2, 0)
                }),
                DropEffect::NestInto {
                    folder_id: "f1".to_string()
                }
            );
            assert!(matches!(
                drop_effect(DropQuery {
                    band,
                    can_sibling: true,
                    ..query(DropTargetKind::Folder, "f1", 1, 1)
                }),
                DropEffect::NestInto { .. }
            ));
        }
    }

    #[test]
    fn a_sibling_the_graph_refuses_is_refused() {
        // The parent's can_nest is the question, and a folder asked to sibling
        // itself has no seam to speak of.
        let edge = DropQuery {
            band: Band::Top,
            can_sibling: false,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(edge), DropEffect::Refused);
        let onto_itself = DropQuery {
            band: Band::Bottom,
            can_sibling: true,
            target_is_held: true,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(onto_itself), DropEffect::Refused);
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

    #[test]
    fn a_band_is_only_an_after_at_the_bottom() {
        assert!(!Band::Top.after());
        assert!(!Band::Middle.after());
        assert!(Band::Bottom.after());
    }
}
