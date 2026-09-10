//! When a book lands on a shelf that already holds it: the ask, and the three
//! answers.
//!
//! The library holds one row per content identity as a rule, and every placing
//! surface leaned on it — an import that found the fingerprint already present
//! resolved to the row it had and filed that, which on a shelf that already
//! held the row was a placement skipped, and to the reader a book that
//! vanished into the shelf it was dropped on. This module replaces the
//! vanishing with a question, and the question has the three answers a file
//! manager teaches everybody to expect:
//!
//!   * **Duplicate** keeps both: the arrival takes a `_1`, `_2`, … name
//!     ([`library_core::book::duplicate_title`]) and lands beside the shelf's
//!     copy — two rows of one file, which the library now allows (its
//!     sanitizer dedupes by id, and every path-keyed writer — the read, the
//!     check, the purge — treats the rows as the twins they are);
//!   * **Replace** seats the arrival in the shelf copy's place: that row goes
//!     (with the side data only it used — the guards below are what make
//!     "only it" precise) and the arrival takes its slot. The sheet asks twice
//!     for this one, because the row going is a resume point and a name the
//!     reader can see;
//!   * **Merge** folds the two rows into one — the shelf's copy survives, the
//!     arrival dissolves into it, and every value follows a named policy in
//!     [`library_core::merge`]: the resume point is the FURTHER of the two (a
//!     merge never sends a reader backwards), names fill gaps, marks union.
//!
//! The same content by a different name is NOT a conflict — a file re-encoded
//! into a second format measures a different fingerprint and simply lands,
//! which is the honest answer: two books that merely rhyme are two books.
//!
//! ## What is a conflict, precisely
//!
//! [`screen`] is the whole rule, and it is per placement rather than per
//! import: the target is a real shelf (the root level has no member list to
//! collide with), the arrival is not already a member (that drop is a
//! reorder), and some member of the target holds the arrival's fingerprint.
//! Every placing surface hands its placements through here BEFORE writing
//! anything — a drag (`arrange::move_many_to_shelf`), a filing
//! (`arrange::file_many`, `arrange::also_show`) and a loose-file import
//! (`import::run_files`) — and each applies the clean half at once, so a drop
//! of ten files with two collisions files eight and asks about two. A watched
//! folder's own rescan never asks: it is the ledger's job to stay quiet, and
//! its placements go through the folder shelf chain rather than a hand.
//!
//! ## The queue
//!
//! One sheet, one question at a time, and a queue behind it: the item at the
//! front is on screen, the count is what the sheet's header says, and the
//! switch offers the same answer for the rest. Cancel stops the remaining
//! questions — the industry's own rule for a copy dialog, where the files
//! already answered keep their answer and the ones not asked are simply not
//! placed. The state lives on [`crate::state::library::LibraryState`] rather
//! than in a component's handle because the raisers are services: an import
//! asks from inside a spawned future that outlived every component.

use std::collections::HashSet;

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use library_core::book::{Book, Origin, add_book, duplicate_title, stem_of};
use library_core::id;
use library_core::merge::merge_books;
use library_core::scan::FoundFile;
use library_core::shelf::{self, Shelf};
use reader_core::format::Format;

use super::arrange::{drop_row, sweep_path};
use crate::state::AppState;

/// What is arriving on a shelf.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A row the library already holds, being moved or filed by hand.
    Move { book_id: String },
    /// A file an import just measured, with no row of its own yet.
    Import { file: FoundFile },
}

/// One intended placement: what arrives, where, and — for a move — where it
/// leaves and which slot the drop pointed at.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub incoming: Incoming,
    pub shelf_id: String,
    /// The shelf a MOVE takes the row off when it lands. `None` for a filing
    /// or an import, which arrive from nowhere the library tracks.
    pub from: Option<String>,
    /// The slot the drop pointed at; `None` appends.
    pub index: Option<usize>,
}

/// One placement that found its own content already on the shelf.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictItem {
    pub placement: Placement,
    /// The shelf member holding the same content — the row the answers act
    /// against, and the survivor of a merge.
    pub existing_id: String,
}

/// The reader's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Keep both: the arrival takes a counter name and lands beside the copy.
    Duplicate,
    /// The shelf's copy goes; the arrival takes its slot and its name on the
    /// shelf. Asked twice — see [`Step::ConfirmReplace`].
    Replace,
    /// Fold the two rows into one, per [`library_core::merge`].
    Merge,
}

/// Which question the sheet is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The three answers.
    Choose,
    /// The second ask a Replace owes: the shelf's copy is about to lose its
    /// name, its resume point and everything only it held, and one click on
    /// the first sheet is not enough for that.
    ConfirmReplace,
}

/// The sheet's whole state: the queue, the step, and the switch.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictAsk {
    /// The queue. The front is the question on screen.
    pub items: Vec<ConflictItem>,
    pub step: Step,
    /// Give the rest of the queue the same answer as this one.
    pub apply_all: bool,
}

impl ConflictAsk {
    /// A fresh ask: the first question, no promises about the rest.
    pub fn new(items: Vec<ConflictItem>) -> Self {
        Self {
            items,
            step: Step::Choose,
            apply_all: false,
        }
    }

    /// The question on screen.
    pub fn current(&self) -> Option<&ConflictItem> {
        self.items.first()
    }

    /// How many questions follow this one — what the "apply to all" switch
    /// and the header's count both read.
    pub fn rest(&self) -> usize {
        self.items.len().saturating_sub(1)
    }
}

/// Milliseconds since the epoch — the library's only clock, the import
/// module's own helper repeated rather than widened for one caller.
fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

// ---------------------------------------------------------------------------
// The screen: what lands now, and what has to ask first.
// ---------------------------------------------------------------------------

/// Split placements into the ones that may land now and the ones the shelf
/// has to ask about. Pure reads — nothing is written here; the caller applies
/// its clean half through its own mechanics and hands the rest to [`raise`].
pub fn screen(
    state: AppState,
    placements: Vec<Placement>,
) -> (Vec<Placement>, Vec<ConflictItem>) {
    let books = state.library.books.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let mut clean = Vec::with_capacity(placements.len());
    let mut conflicts = Vec::new();
    for placement in placements {
        match blocks(&books, &shelves, &placement) {
            Some(existing_id) => conflicts.push(ConflictItem {
                placement,
                existing_id,
            }),
            None => clean.push(placement),
        }
    }
    (clean, conflicts)
}

/// The member that makes a placement ask, when one does.
///
/// Split out of [`screen`] because it is the whole of the rule and a rule
/// this load-bearing is one a test can hold: the root level never conflicts
/// (it has no member list), an arrival already on the shelf never conflicts
/// (that drop is a reorder, and asking would be asking about a move that is
/// not one), and the match is by CONTENT — a member whose fingerprint equals
/// the arrival's, whatever the two are called.
fn blocks(books: &[Book], shelves: &[Shelf], placement: &Placement) -> Option<String> {
    if placement.shelf_id == shelf::ALL_SHELF {
        return None;
    }
    let target = shelves.iter().find(|s| s.id == placement.shelf_id)?;
    let fp = match &placement.incoming {
        Incoming::Move { book_id } => {
            if target.books.iter().any(|m| m == book_id) {
                return None;
            }
            books.iter().find(|b| &b.id == book_id)?.fp
        }
        Incoming::Import { file } => file.fp,
    };
    target
        .books
        .iter()
        .filter_map(|member| books.iter().find(|b| &b.id == member))
        .find(|book| book.fp == fp)
        .map(|book| book.id.clone())
}

/// Put questions in front of the reader. A sheet already up takes the items
/// onto its queue rather than being replaced: two drops in flight owe two
/// answers, and a raise that dropped the first question would be a placement
/// vanishing exactly the way this module exists to stop.
pub fn raise(state: AppState, items: Vec<ConflictItem>) {
    if items.is_empty() {
        return;
    }
    let open = state.library.conflict_open.get_untracked();
    let mut ask = if open {
        state
            .library
            .conflict
            .get_untracked()
            .unwrap_or_else(|| ConflictAsk::new(Vec::new()))
    } else {
        ConflictAsk::new(Vec::new())
    };
    ask.items.extend(items);
    state.library.conflict.set(Some(ask));
    if !open {
        state.library.conflict_open.set(true);
    }
}

// ---------------------------------------------------------------------------
// The sheet's buttons.
// ---------------------------------------------------------------------------

/// One of the three rows. Replace does not resolve here — it moves the sheet
/// to its second ask, which is [`confirm_replace`]'s job.
pub fn choose(state: AppState, choice: Choice) {
    if choice == Choice::Replace {
        set_step(state, Step::ConfirmReplace);
        return;
    }
    apply_choice(state, choice);
}

/// The second ask's yes: the shelf's copy goes.
pub fn confirm_replace(state: AppState) {
    apply_choice(state, Choice::Replace);
}

/// The second ask's no: back to the three rows, nothing done.
pub fn back_to_choices(state: AppState) {
    set_step(state, Step::Choose);
}

/// The queue's switch: the next answer is also the last one.
pub fn set_apply_all(state: AppState, on: bool) {
    state.library.conflict.update(|ask| {
        if let Some(ask) = ask {
            ask.apply_all = on;
        }
    });
}

/// Cancel — the sheet's, the backdrop's and the Escape key's one write. The
/// question on screen and every one behind it are skipped: the placements
/// already answered keep their answers, and the rest simply do not land,
/// which is what a file manager's copy dialog has always meant by Cancel.
pub fn cancel(state: AppState) {
    state.library.conflict_open.set(false);
    state.library.conflict.set(None);
}

fn set_step(state: AppState, step: Step) {
    state.library.conflict.update(|ask| {
        if let Some(ask) = ask {
            ask.step = step;
        }
    });
}

/// Resolve the front of the queue — and behind it too, when the switch is on.
fn apply_choice(state: AppState, choice: Choice) {
    let Some(mut ask) = state.library.conflict.get_untracked() else {
        return;
    };
    if ask.apply_all {
        for item in std::mem::take(&mut ask.items) {
            resolve(state, &item, choice);
        }
        cancel(state);
        return;
    }
    let Some(item) = ask.items.first().cloned() else {
        cancel(state);
        return;
    };
    ask.items.remove(0);
    resolve(state, &item, choice);
    if ask.items.is_empty() {
        cancel(state);
    } else {
        ask.step = Step::Choose;
        state.library.conflict.set(Some(ask));
    }
}

fn resolve(state: AppState, item: &ConflictItem, choice: Choice) {
    match choice {
        Choice::Duplicate => duplicate(state, item),
        Choice::Replace => replace(state, item),
        Choice::Merge => merge(state, item),
    }
}

// ---------------------------------------------------------------------------
// The three answers.
// ---------------------------------------------------------------------------

/// Keep both: the arrival takes the first free counter name and lands.
fn duplicate(state: AppState, item: &ConflictItem) {
    match &item.placement.incoming {
        Incoming::Move { book_id } => {
            // The row went while the sheet was up (a removal from another
            // surface, a purge): there is nothing to rename or to land.
            if book_by_id(state, book_id).is_none() {
                return;
            }
            let name = duplicate_name(state, item);
            if name.is_empty() {
                return;
            }
            state.library.books.update(|books| {
                if let Some(row) = books.iter_mut().find(|b| &b.id == book_id) {
                    row.title = Some(name);
                }
            });
            land(state, &item.placement, book_id, item.placement.index);
        }
        Incoming::Import { file } => {
            let now = now_ms();
            let name = duplicate_name(state, item);
            let book = Book {
                title: Some(name),
                ..Book::new(
                    id::next_id(now),
                    file.fp,
                    file.format().unwrap_or(Format::Pdf),
                    Origin::Linked {
                        src: file.path.clone(),
                    },
                    now,
                )
            };
            let new_id = book.id.clone();
            state.library.books.update(|books| books.push(book));
            land(state, &item.placement, &new_id, item.placement.index);
            // The twin may read from an address the cache has no art for.
            super::covers::backfill_missing(state);
        }
    }
    crate::storage::persist_library(state.library);
}

/// The shelf's copy goes; the arrival takes its seat — the slot it held, not
/// the tail, because a replace is an overwrite and an overwrite stays where
/// the thing it replaced was — and every OTHER shelf it was filed on, so a
/// replace never silently takes a book off shelves the question never
/// mentioned. What it does take is the row's own values: its name, its resume
/// point and the side data only its address held, which is what the second
/// ask is for.
fn replace(state: AppState, item: &ConflictItem) {
    let seat = member_slot(state, &item.placement.shelf_id, &item.existing_id);
    let inherited = shelves_containing(state, &item.existing_id);
    let dropped = drop_row(state, &item.existing_id);
    let seat = seat.or(item.placement.index);
    let arrival_id = match &item.placement.incoming {
        Incoming::Move { book_id } => {
            if book_by_id(state, book_id).is_none() {
                return;
            }
            book_id.clone()
        }
        Incoming::Import { file } => {
            let now = now_ms();
            let book = Book::new(
                id::next_id(now),
                file.fp,
                file.format().unwrap_or(Format::Pdf),
                Origin::Linked {
                    src: file.path.clone(),
                },
                now,
            );
            let new_id = book.id.clone();
            state.library.books.update(|books| books.push(book));
            new_id
        }
    };
    land(state, &item.placement, &arrival_id, seat);
    state.library.shelves.update(|shelves| {
        for shelf_id in &inherited {
            if let Some(shelf) = shelves.iter_mut().find(|s| &s.id == shelf_id) {
                shelf::shelf_add(shelf, &arrival_id);
            }
        }
    });
    // The seat's old occupant may have held the only art for an address the
    // arrival also reads; the queue renders whatever is missing.
    super::covers::backfill_missing(state);
    crate::storage::persist_library(state.library);
    if dropped.is_some() {
        crate::storage::persist_covers(state.library);
    }
}

/// Fold the arrival into the shelf's copy: the copy survives with its id and
/// its memberships, the arrival's row (when it has one) dissolves and its
/// memberships transfer, and every value — the row's and the side data's —
/// follows its policy.
fn merge(state: AppState, item: &ConflictItem) {
    let Some(existing) = book_by_id(state, &item.existing_id) else {
        // The shelf's copy went while the sheet was up: the conflict it was
        // half of is gone, and the arrival is the clean placement it was
        // before the question.
        land_clean(state, &item.placement);
        crate::storage::persist_library(state.library);
        return;
    };
    let now = now_ms();
    let (incoming, incoming_id) = match &item.placement.incoming {
        Incoming::Move { book_id } => {
            let Some(row) = book_by_id(state, book_id) else {
                return;
            };
            (row, Some(book_id.clone()))
        }
        // No row yet: the hypothetical one the fold consumes. Its blank id
        // never escapes — `merge_books` keeps the survivor's.
        Incoming::Import { file } => (
            Book::new(
                String::new(),
                file.fp,
                file.format().unwrap_or(Format::Pdf),
                Origin::Linked {
                    src: file.path.clone(),
                },
                now,
            ),
            None,
        ),
    };

    let merged = merge_books(&existing, &incoming);
    // The addresses the fold leaves behind: whichever of the two rows' the
    // merged one does not read from. Their marks and their art travel to the
    // survivor's address first — the union is the point of a merge — and what
    // is left at them afterwards is swept when no row reads it any more.
    let survivor = merged.path().to_string();
    let mut left_behind: Vec<(String, bool)> = Vec::new();
    for book in [&existing, &incoming] {
        let path = book.path();
        if path != survivor && !left_behind.iter().any(|(p, _)| p == path) {
            left_behind.push((path.to_string(), book.origin.is_stored()));
        }
    }

    state.library.books.update(|books| {
        if let Some(row) = books.iter_mut().find(|b| b.id == existing.id) {
            *row = merged;
        }
    });
    for (path, _) in &left_behind {
        union_gloss(path, &survivor);
        transfer_cover(state, path, &survivor);
    }
    if let Some(incoming_id) = &incoming_id {
        // The move was taking the arrival off `from`; the fold takes it off
        // every shelf instead, and every OTHER shelf it sat on is a shelf the
        // survivor now sits on too — memberships are values, and the policy
        // for a value both rows held is the union.
        transfer_memberships(state, incoming_id, &existing.id, item.placement.from.as_deref());
        drop_row(state, incoming_id);
    }
    for (path, was_stored) in &left_behind {
        // The arrival's address was swept by `drop_row` when it had a row;
        // the survivor's OLD address (a dead one the fold healed past) has no
        // removal to ride and is swept here. Idempotent either way.
        sweep_path(state, path, *was_stored);
    }
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

// ---------------------------------------------------------------------------
// The pieces the answers share.
// ---------------------------------------------------------------------------

fn book_by_id(state: AppState, id: &str) -> Option<Book> {
    state
        .library
        .books
        .with_untracked(|books| books.iter().find(|b| b.id == id).cloned())
}

fn member_slot(state: AppState, shelf_id: &str, book_id: &str) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .and_then(|s| s.books.iter().position(|m| m == book_id))
    })
}

/// The ids of every shelf a row is filed on, in shelf order.
fn shelves_containing(state: AppState, book_id: &str) -> Vec<String> {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.books.iter().any(|m| m == book_id))
            .map(|s| s.id.clone())
            .collect()
    })
}

/// Every name the library shows — the pool the duplicate counter counts
/// against, so a minted name collides with nothing on any shelf, not merely
/// with nothing on this one.
fn titles_in_use(state: AppState) -> HashSet<String> {
    state
        .library
        .books
        .with_untracked(|books| books.iter().map(|b| b.title()).collect())
}

/// The name a Duplicate gives the arrival — minted live, because the sheet
/// PROMISES it on the row the reader clicks and a promise computed before the
/// queue moved could name a book that no longer exists.
///
/// `base` is the arrival's own name (a moved row's title, an import's stem):
/// the counter extends what arrived, the way a file manager renames the file
/// it is copying rather than the one already there. A moved row's own name is
/// vacated by the rename, so it does not block its own counter.
pub fn duplicate_name(state: AppState, item: &ConflictItem) -> String {
    let mut in_use = titles_in_use(state);
    let base = match &item.placement.incoming {
        Incoming::Move { book_id } => {
            let Some(book) = book_by_id(state, book_id) else {
                return String::new();
            };
            in_use.remove(&book.title());
            book.title()
        }
        Incoming::Import { file } => stem_of(&file.path),
    };
    duplicate_title(&base, &in_use)
}

/// Land one placement: off `from` when the move named one, onto the shelf at
/// the slot the answer gave. The whole of a deferred move — the same two
/// edits [`super::arrange::move_many_to_shelf`] makes for the half of a drag
/// that never had to ask.
fn land(state: AppState, placement: &Placement, book_id: &str, index: Option<usize>) {
    state.library.shelves.update(|shelves| {
        if let Some(from) = placement.from.as_deref().filter(|id| *id != placement.shelf_id)
            && let Some(shelf) = shelves.iter_mut().find(|s| s.id == from)
        {
            shelf::forget(&mut shelf.books, book_id);
        }
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == placement.shelf_id) {
            shelf::place(&mut shelf.books, book_id, index);
        }
    });
}

/// Land a placement that no longer has anything to collide with: the move
/// lands as the move it was, the import files through [`add_book`] — which
/// may resolve it to a row the library already holds elsewhere, the rule an
/// import has always followed.
fn land_clean(state: AppState, placement: &Placement) {
    match &placement.incoming {
        Incoming::Move { book_id } => {
            if book_by_id(state, book_id).is_some() {
                land(state, placement, book_id, placement.index);
            }
        }
        Incoming::Import { file } => {
            let now = now_ms();
            let book = Book::new(
                id::next_id(now),
                file.fp,
                file.format().unwrap_or(Format::Pdf),
                Origin::Linked {
                    src: file.path.clone(),
                },
                now,
            );
            let mut placed_id = String::new();
            state
                .library
                .books
                .update(|books| placed_id = add_book(books, book));
            land(state, placement, &placed_id, placement.index);
            super::covers::backfill_missing(state);
        }
    }
}

/// Give the survivor every shelf the dissolving row sat on, except the one
/// the move was taking it off — a merge into a shelf the arrival was LEAVING
/// would undo the leaving.
fn transfer_memberships(
    state: AppState,
    dissolving_id: &str,
    survivor_id: &str,
    leaving: Option<&str>,
) {
    state.library.shelves.update(|shelves| {
        for shelf in shelves.iter_mut() {
            if leaving == Some(shelf.id.as_str()) {
                continue;
            }
            if shelf.books.iter().any(|m| m == dissolving_id) {
                shelf::shelf_add(shelf, survivor_id);
            }
        }
    });
}

/// Both addresses' marks under the survivor's. The marks keep their ids, and
/// the AI answers ride the ids — a mark that travels arrives with the answer
/// it already had, and a spot both rows marked keeps the survivor's mark.
fn union_gloss(from: &str, into: &str) {
    let all = crate::storage::load_gloss();
    let Some(extra) = all.get(from).filter(|marks| !marks.is_empty()) else {
        return;
    };
    let base = all.get(into).cloned().unwrap_or_default();
    let union = union_marks(&base, extra);
    crate::storage::persist_gloss(into, &union);
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

/// Move the address's cached art to the survivor's address when the survivor
/// has none of its own — a merged book should not flash back to a rendered
/// plate for an address it just rendered one under.
fn transfer_cover(state: AppState, from: &str, into: &str) {
    state.library.covers.update(|covers| {
        if covers.contains_key(into) {
            return;
        }
        if let Some(cover) = covers.get(from).cloned() {
            covers.insert(into.to_string(), cover);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::union_marks;
    use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};

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
