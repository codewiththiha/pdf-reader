//! The side tables a merge folds: everything two rows of one book hold that
//! is NOT on the row — the data keyed by an ADDRESS rather than by an id.
//!
//! [`library_core::merge`] owns the fold of the rows themselves, field by
//! field, and answers with the merged book plus a [`MergeNotes`] whose resume
//! half it can compute alone. The marks half — the gloss highlights, and the
//! AI answers riding their ids — lives in the app's own storage, and the
//! cached cover art lives on the library state; folding those is this
//! module's job, and it fills the notes' mark counts as it goes.
//!
//! ## The registry
//!
//! One merger per side table, listed once in [`SIDE_MERGERS`]: a table the
//! library grows later (a per-book position cache, an annotation export) is
//! one function added to the list and nothing else changes — [`merge_side_tables`]
//! walks the list, and the caller in `crate::services::library::conflict`
//! never learns what the tables are. Each merger takes the address the fold
//! leaves behind and the survivor's, and reports what it kept into the notes.
//!
//! ## Removal stays with the sweep
//!
//! A merger MOVES data to the survivor's address; it never deletes the old
//! one. The removal is `crate::services::library::arrange::sweep_path`'s,
//! which holds the twin guard this module must not duplicate: a third row —
//! a duplicate the reader kept earlier — may read the same address, and its
//! highlights and its art survive with it. The merge's `drop_row` rides that
//! sweep, and an address no remaining row reads is cleaned there, once.

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use library_core::merge::MergeNotes;

use crate::state::AppState;

/// One side table's fold: `(from_path, into_path)` plus the notes to report
/// into. A `fn` rather than a trait because the registry is a const list the
/// merge walks — the same shape [`library_core::merge::POLICIES`] gives the
/// row fields.
type SideMerger = fn(AppState, &str, &str, &mut MergeNotes);

/// Every side table a merge folds, in the order they are folded. A new
/// address-keyed table joins here and in nowhere else. The gloss has one more
/// caller than the registry — [`fold_gloss`], for the fold that moves a
/// survivor's own marks out of its id-keyed store — and it is the same
/// function the registry walks, so the two cannot disagree.
const SIDE_MERGERS: &[SideMerger] = &[gloss_merger, cover_merger];

/// Fold every side table from one left-behind address into the survivor's,
/// counting what was kept into `notes`. Called once per address the fold
/// leaves behind — a merge of two rows at two addresses leaves one, a merge
/// that also healed a dead address leaves two.
pub fn merge_side_tables(state: AppState, from: &str, into: &str, notes: &mut MergeNotes) {
    for merger in SIDE_MERGERS {
        merger(state, from, into, notes);
    }
}

/// The dry run of [`gloss_merger`]: the same counts over the same tables,
/// with no writes at all. What the conflict sheet's Merge row promises from —
/// the promise and the fold read one rule, so they cannot drift.
pub fn count_marks(from: &[String], into: &str, notes: &mut MergeNotes) {
    let all = crate::storage::load_gloss();
    let mut mine = all.get(into).cloned().unwrap_or_default();
    notes.marks_kept = mine.len();
    for path in from {
        let Some(theirs) = all.get(path).filter(|marks| !marks.is_empty()) else {
            continue;
        };
        let union = union_marks(&mine, theirs);
        notes.marks_added += union.len() - mine.len();
        mine = union;
    }
}

/// Both addresses' marks under the survivor's. The marks keep their ids, and
/// the AI answers ride the ids — a mark that travels arrives with the answer
/// it already had, and a spot both rows marked keeps the survivor's mark.
fn gloss_merger(_state: AppState, from: &str, into: &str, notes: &mut MergeNotes) {
    fold_gloss(from, into, notes)
}

/// [`gloss_merger`] by itself, for the one fold that is not between two
/// addresses: a survivor that was a book of its own kept its marks under a key
/// carrying its id ([`library_core::book::Book::gloss_key`]), and a merge ends
/// that — one book reads the address's list like every other book
/// ([`library_core::merge::Policy::Folded`]). The count it reports into `notes`
/// is the same one [`count_marks`] promises, so the sheet's dry run and the
/// fold cannot drift over it either.
pub fn fold_gloss(from: &str, into: &str, notes: &mut MergeNotes) {
    let all = crate::storage::load_gloss();
    let mine = all.get(into).cloned().unwrap_or_default();
    // `mine` already holds what an earlier pass moved, so the count of what
    // was ORIGINALLY the survivor's is what is left after subtracting those.
    notes.marks_kept = mine.len().saturating_sub(notes.marks_added);
    let Some(theirs) = all.get(from).filter(|marks| !marks.is_empty()) else {
        return;
    };
    let union = union_marks(&mine, theirs);
    notes.marks_added += union.len() - mine.len();
    crate::storage::persist_gloss(into, &union);
}

/// Move the address's cached art to the survivor's address when the survivor
/// has none of its own — a merged book should not flash back to a rendered
/// plate for an address it just rendered one under. The survivor's own art
/// wins whenever it has any, which is what makes the shelf's cover the same
/// one the reader last saw.
fn cover_merger(state: AppState, from: &str, into: &str, _notes: &mut MergeNotes) {
    state.library.covers.update(|covers| {
        if covers.contains_key(into) {
            return;
        }
        if let Some(cover) = covers.get(from).cloned() {
            covers.insert(into.to_string(), cover);
        }
    });
}

/// The union of two mark lists by spot identity: everything `base` holds, in
/// its order, plus every mark of `extra` denoting a spot `base` has not
/// marked. [`GlossMark::same_spot`] is the identity — the same rule capture
/// dedupes by and a re-click toggles by, so a merged shelf agrees with the
/// page it renders on about what "the same mark" is.
pub fn union_marks(base: &[GlossMark], extra: &[GlossMark]) -> Vec<GlossMark> {
    let mut union: Vec<GlossMark> = base.to_vec();
    for mark in extra {
        if !union.iter().any(|kept| kept.same_spot(mark)) {
            union.push(mark.clone());
        }
    }
    union
}

#[cfg(test)]
mod tests {
    use super::union_marks;
    use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};
    use library_core::book::{Book, Fingerprint, Origin};
    use library_core::merge::merge_books;
    use reader_core::format::Format;

    fn mark(id: &str, word: &str, page: u32, x: f64) -> GlossMark {
        GlossMark {
            id: id.to_string(),
            word: word.to_string(),
            context: String::new(),
            anchor: PageAnchor {
                page,
                rect: GlossBox { x, y: 10.0, w: 40.0, h: 12.0, r: 2.0 },
            },
        }
    }

    fn book(id: &str, path: &str, page: u32) -> Book {
        let mut book = Book::new(
            id.to_string(),
            Fingerprint { size: 10, mtime_ms: 5, head_hash: 9 },
            Format::Markdown,
            Origin::Linked { src: path.to_string() },
            10,
        );
        book.page = page;
        book
    }

    #[test]
    fn merge_takes_the_further_page_and_unions_every_mark() {
        // The row half: the survivor resumes where the reader got furthest,
        // and the notes say the point travelled.
        let (merged, notes) = merge_books(&book("keep", "/one/dune.md", 12), &book("gone", "/two/dune.md", 240));
        assert_eq!(merged.page, 240);
        assert_eq!(merged.id, "keep");
        assert!(notes.resume_from_incoming);
        assert_eq!(notes.resume_page, 240);
        // The side-table half: every mark either address held survives the
        // union, one per spot — the base's mark wins a spot both held.
        let mine = vec![mark("g1", "spice", 4, 100.0), mark("g2", "worm", 9, 20.0)];
        let theirs = vec![
            mark("g9", "spice", 4, 100.4), // the same spot the base marked
            mark("g7", "arrakis", 4, 300.0),
            mark("g8", "worm", 12, 20.0),
        ];
        let union = union_marks(&mine, &theirs);
        let ids: Vec<&str> = union.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["g1", "g2", "g7", "g8"], "every spot either side held");
        assert_eq!(union.len(), mine.len() + 2, "two marks travelled, one was already there");
    }

    #[test]
    fn the_union_keeps_both_sides_and_one_of_a_spot() {
        let base = vec![mark("g1", "spice", 4, 100.0), mark("g2", "worm", 9, 20.0)];
        let extra = vec![
            // The same spot the base marked, under its own id: one mark
            // survives, and it is the base's — the answers ride the ids.
            mark("g9", "spice", 4, 100.4),
            // A spot the base has not marked, on the same page and on
            // another: both travel.
            mark("g7", "arrakis", 4, 300.0),
            mark("g8", "worm", 12, 20.0),
        ];
        let union = union_marks(&base, &extra);
        let ids: Vec<&str> = union.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["g1", "g2", "g7", "g8"]);
        // Sub-pixel drift on the same page is the same spot — `same_spot`'s
        // own tolerance, which the union inherits rather than reinvents.
        assert_eq!(union_marks(&[], &extra).len(), 3);
        assert!(union_marks(&base, &[]).iter().eq(&base));
    }

    #[test]
    fn the_same_word_in_two_places_is_two_marks() {
        // "spice" on page 4 and on page 40 are two glossed spots: the
        // identity is the word AND the anchor, never the word alone.
        let base = vec![mark("g1", "spice", 4, 100.0)];
        let extra = vec![mark("g2", "spice", 40, 100.0)];
        assert_eq!(union_marks(&base, &extra).len(), 2);
    }
}
