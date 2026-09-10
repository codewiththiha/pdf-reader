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
//!     for this one WHEN the copy takes something with it — a resume point or
//!     highlights at an address the arrival does not read from — because one
//!     click is not enough for a loss the reader can see; a replace between
//!     two rows of one address loses only the row and its name, which the
//!     first sheet already said, and resolves on the spot;
//!   * **Merge** folds the two rows into one — the shelf's copy survives, the
//!     arrival dissolves into it, and every value follows a named policy in
//!     [`library_core::merge`]: the resume point is the FURTHER of the two (a
//!     merge never sends a reader backwards), names fill gaps, marks union.
//!     The fold answers with a [`library_core::merge::MergeNotes`] beside the
//!     merged row, and the address-keyed side tables — the marks, the art —
//!     ride the registry in `merge_side`, which fills the notes' mark counts
//!     as it goes. The sheet's Merge row promises those counts BEFORE the
//!     click, from a dry run of the same fold ([`merge_note`]).
//!
//! The same content by a different name is NOT a conflict — a file re-encoded
//! into a second format measures a different fingerprint and simply lands,
//! which is the honest answer: two books that merely rhyme are two books.
//!
//! ## The question that is not about content
//!
//! One collision is different, and asking it the three questions above asks
//! the wrong thing: the SAME FILE arriving at an address the library already
//! reads it from. Import "dune.pdf" twice and the second arrival is not a
//! copy of the shelf's book that might be worth replacing or folding — it IS
//! that file, and the two rows would share its address, and with it the
//! highlights keyed by the address, the resume point every writer at the
//! address updates, and the removal that sweeps the address when the last row
//! leaves it. The reader is not being asked what to do about a duplicate; they
//! are being asked whether they want one book or two. So [`kind_of`] splits the
//! queue by [`ConflictKind`], and a [`ConflictKind::SameLinkedFile`] item gets
//! [`LinkedFileChoice`]'s three answers on the same sheet, in the same queue,
//! behind the same switch:
//!
//!   * **Already imported** places nothing: it reveals the book the reader
//!     already has ([`super::reveal`]), which is the answer that says "I did
//!     not mean to add anything";
//!   * **As new** is [`Choice::Duplicate`]'s row operation plus the one mark
//!     that makes the arrival a book of its own
//!     ([`library_core::book::Book::independent`]): its highlights live under a
//!     key carrying its id, its resume point is written by itself, and removing
//!     either row takes nothing from the other;
//!   * **Linked** is [`Choice::Duplicate`] exactly, and says so — two rows that
//!     go on sharing everything an address holds.
//!
//! The rule is the address and nothing else, which is why a stored copy never
//! asks it: a stored book's address is the app's own and no import can arrive
//! at it. Independence is a mark on a row rather than a second kind of row, so
//! everything the library already knew about twins still holds — the ledger
//! names a shared row before a private one, an import resolves to a shared row
//! and never to a private one, and a fold ends the mark
//! ([`library_core::merge::Policy::Folded`]).
//!
//! ## What is a conflict, precisely
//!
//! [`screen`] is the whole rule, and it is per placement rather than per
//! import. Every level has a member list — a shelf's own, and at the root
//! level the unfiled books, which the "All" list renders and which used to
//! make the root a level a duplicate could vanish into — and the rule is the
//! same on all of them: the arrival is not already a member of the target
//! (that drop is a reorder), and some member holds the arrival's fingerprint.
//! Every placing surface hands its placements through here BEFORE writing
//! anything — a drag (`arrange::move_many_to_shelf`), a lift out to the root
//! (`arrange::unfile_books`), a filing (`arrange::file_many`,
//! `arrange::also_show`) and a loose-file import (`import::run_files`) — and
//! each applies the clean half at once, so a drop of ten files with two
//! collisions files eight and asks about two. A folder filed inside another
//! asks too, through [`screen_nest`]: the nesting itself writes no
//! membership, but the parent's own list may already hold the content one of
//! the folder's books carries, and the pair is the sheet's question like any
//! other. A watched folder's own rescan never asks: it is the ledger's job to
//! stay quiet, and its placements go through the folder shelf chain rather
//! than a hand.
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

use library_core::book::{Book, Origin, add_book, duplicate_title, stem_of};
use library_core::id;
use library_core::merge::{MergeNotes, merge_books};
use library_core::scan::FoundFile;
use library_core::shelf::{self, Shelf};
use library_core::text::plural;
use reader_core::format::Format;

use super::arrange::{drop_row, sweep_path};
use super::merge_side;
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
    /// Which question this is, and so which three answers the sheet asks.
    pub kind: ConflictKind,
}

/// Which question a blocked placement is. The sheet changes its whole
/// vocabulary on this, so it is a value rather than something the view
/// re-derives from the two rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// One content at two addresses: the file the arrival reads is a copy of
    /// the one the shelf's row reads, and nothing else is known. The three
    /// file-manager answers ([`Choice`]) are the question this was written
    /// for.
    Fingerprint,
    /// The same ADDRESS arriving twice — a file imported onto a level that
    /// already reads it, or a row dragged onto a level holding its own twin.
    /// Nothing about the content is in doubt, so the three answers above are
    /// the wrong three: what is in question is whether the reader wants one
    /// book or two, which is [`LinkedFileChoice`].
    SameLinkedFile,
}

/// The same-address question's answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedFileChoice {
    /// Nothing to place — take the reader to the book they already have: its
    /// shelf, then its card, lit ([`super::reveal::reveal_book`]).
    GoToExisting,
    /// A book of its own: the arrival takes a counter name and the
    /// independence mark, so its highlights, its resume point and its removal
    /// are its own ([`library_core::book::Book::independent`]).
    AsNew,
    /// Two rows, one book: they share the address, and with it the highlights,
    /// the position and the removal. The [`Choice::Duplicate`] answer wearing
    /// this question's words.
    LinkShared,
}

/// The reader's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Keep both: the arrival takes a counter name and lands beside the copy.
    Duplicate,
    /// The shelf's copy goes; the arrival takes its slot and its name on the
    /// shelf. Asks twice when the copy takes something with it — see
    /// [`Step::ConfirmReplace`].
    Replace,
    /// Fold the two rows into one, per [`library_core::merge`].
    Merge,
}

/// Which question the sheet is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The three answers.
    Choose,
    /// The second ask a Replace owes WHEN the shelf's copy takes something
    /// the arrival cannot inherit — its resume point, or its highlights at an
    /// address no surviving row reads from — and one click on the first sheet
    /// is not enough for that. A replace that loses only the row and its name
    /// never comes here: the first sheet said exactly that much already.
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

    /// Whether the question on screen is the same-address one, which has its
    /// own three answers.
    ///
    /// Derived from the item at the front rather than stored beside it: a
    /// queue grows by extending whatever sheet is already up ([`raise`]), so a
    /// flag set at construction would keep answering for the item that opened
    /// the sheet long after the sheet had moved on to a different question.
    pub fn linked_flow(&self) -> bool {
        self.current()
            .is_some_and(|item| item.kind == ConflictKind::SameLinkedFile)
    }

    /// Whether the whole queue asks the question on screen. The switch's
    /// precondition: "the same answer for the rest" is a sentence that means
    /// something only while the rest are being asked the same thing, and a
    /// queue of two questions with three answers each and three with another
    /// three is not one question asked five times.
    pub fn uniform(&self) -> bool {
        match self.current() {
            Some(item) => self.items.iter().all(|other| other.kind == item.kind),
            None => false,
        }
    }
}

/// Milliseconds since the epoch — the library's only clock, the import
/// module's own helper repeated rather than widened for one caller. Off wasm
/// — the host test runs this crate gets from `cargo test --workspace` — the
/// clock is inert rather than a panic: the wasm-bindgen stubs abort when
/// called natively, and stamps nobody persists are fine at zero. The ids
/// minted from it stay unique regardless, on [`library_core::id`]'s own
/// counter.
fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
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
            Some(existing_id) => {
                let kind = kind_of(&books, &placement, &existing_id);
                conflicts.push(ConflictItem {
                    placement,
                    existing_id,
                    kind,
                });
            }
            None => clean.push(placement),
        }
    }
    (clean, conflicts)
}

/// Which question a blocked placement is: the address decides.
///
/// An arrival reading from the very address the shelf's copy reads from is the
/// same FILE arriving twice — one book the library already holds, and the only
/// thing to ask is whether the reader wants a second one beside it. Two rows
/// of one content at two ADDRESSES are the other question, the one the three
/// file-manager answers were written for, because there the arrival's own file
/// is a thing the reader has and the shelf's copy is a thing the reader has
/// elsewhere.
///
/// Pure, and split out of [`screen`] for the reason [`blocks`] is: the sheet
/// changes its whole vocabulary on this answer, so it is one a test can hold.
fn kind_of(books: &[Book], placement: &Placement, existing_id: &str) -> ConflictKind {
    let incoming = match &placement.incoming {
        Incoming::Move { book_id } => books
            .iter()
            .find(|b| &b.id == book_id)
            .map(|b| b.path().to_string()),
        Incoming::Import { file } => Some(file.path.clone()),
    };
    let existing = books
        .iter()
        .find(|b| b.id == existing_id)
        .map(|b| b.path().to_string());
    match (incoming, existing) {
        // A row that went while the sheet was being raised has no address to
        // compare, and the answers below resolve that case on their own.
        (Some(incoming), Some(existing)) if incoming == existing => ConflictKind::SameLinkedFile,
        _ => ConflictKind::Fingerprint,
    }
}

/// A folder filed inside another asks too.
///
/// The nesting itself writes no membership — the folder's shelf hangs inside
/// the parent and keeps its own member list — but the parent may ALREADY hold
/// the content one of the folder's direct members carries, and that pair is
/// the sheet's question like any other: without it, the duplicate sits
/// silently inside the parent, invisible at the parent's own level and
/// discoverable only by drilling in. Called after a successful reparent
/// (`arrange::nest_shelf`, `arrange::nest_many`), never for a nest at the
/// root: the root is a level, and the folder's books were already rows of it.
///
/// The clean half needs no action here — that is the whole difference from
/// [`screen`]'s other callers: a member that does not collide is exactly
/// where the nesting already put it. The collisions go on the queue, and
/// their answers are the usual three, resolved against the parent's copy:
/// Duplicate renames the folder's book and files it beside that copy,
/// Replace seats the folder's book in the copy's place, Merge folds it in.
pub fn screen_nest(state: AppState, moved: &[String], target: &str) {
    if target == shelf::ALL_SHELF || moved.is_empty() {
        return;
    }
    let placements = state
        .library
        .shelves
        .with_untracked(|shelves| nest_placements(shelves, moved, target));
    if placements.is_empty() {
        return;
    }
    let (_clean, conflicts) = screen(state, placements);
    raise(state, conflicts);
}

/// The placements a nest owes the screen: every direct member of every moved
/// shelf that the new parent does not itself already hold, as a Move onto the
/// parent. Pure so a test can hold the rule in its hands — [`screen_nest`] is
/// this over the live shelves plus the raise.
///
/// The parent's OWN members are skipped rather than screened: a book already
/// on the parent's list is a member, not an arrival, exactly as a drop of it
/// back onto its own shelf is a reorder. A book held twice among the moved
/// folders is screened once: one arrival, one question.
pub fn nest_placements(shelves: &[Shelf], moved: &[String], target: &str) -> Vec<Placement> {
    let Some(holder) = shelves.iter().find(|s| s.id == target) else {
        return Vec::new();
    };
    let mut placements: Vec<Placement> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for book_id in moved
        .iter()
        .filter_map(|id| shelves.iter().find(|s| s.id == *id))
        .flat_map(|s| s.books.iter().cloned())
    {
        if holder.books.contains(&book_id) || seen.contains(&book_id) {
            continue;
        }
        seen.push(book_id.clone());
        placements.push(Placement {
            incoming: Incoming::Move { book_id },
            shelf_id: target.to_string(),
            from: None,
            index: None,
        });
    }
    placements
}

/// Whether a row is filed on any real shelf. What "at the root" means for a
/// book: the root level's member list is the unfiled rows, so a row no shelf
/// holds is a row the root shows — and a twin arriving there collides with it
/// like an arrival on any shelf collides with a member.
fn is_shelved(shelves: &[Shelf], book_id: &str) -> bool {
    shelves.iter().any(|s| s.books.iter().any(|m| m == book_id))
}

/// The member that makes a placement ask, when one does.
///
/// Split out of [`screen`] because it is the whole of the rule and a rule
/// this load-bearing is one a test can hold: every level has a member list —
/// a shelf's own, and at the root the unfiled rows — an arrival already on
/// the list never conflicts (that drop is a reorder, and asking would be
/// asking about a move that is not one), and the match is by CONTENT — a
/// member whose fingerprint equals the arrival's, whatever the two are
/// called. Same name with a different fingerprint lands silently: two books
/// that merely rhyme are two books.
fn blocks(books: &[Book], shelves: &[Shelf], placement: &Placement) -> Option<String> {
    let fp = match &placement.incoming {
        Incoming::Move { book_id } => {
            let in_target = shelves
                .iter()
                .find(|s| s.id == placement.shelf_id)
                .is_some_and(|s| s.books.iter().any(|m| m == book_id));
            let in_root = !is_shelved(shelves, book_id);
            match (placement.shelf_id == shelf::ALL_SHELF, in_root, in_target) {
                // A row already on the list it is dropped on is a reorder,
                // not an arrival — at the root as much as on a shelf.
                (true, true, _) | (false, _, true) => return None,
                _ => books.iter().find(|b| &b.id == book_id)?.fp,
            }
        }
        Incoming::Import { file } => file.fp,
    };
    if placement.shelf_id == shelf::ALL_SHELF {
        // The root's members are the unfiled rows. A shelved twin does NOT
        // collide here: it is not on this level, and an import landing
        // beside it as its own row is the honest landing — resolving it to
        // the shelved row instead would land nothing the reader can see,
        // which is the vanishing this rule exists to stop.
        return books
            .iter()
            .find(|b| b.fp == fp && !is_shelved(shelves, &b.id))
            .map(|b| b.id.clone());
    }
    let target = shelves.iter().find(|s| s.id == placement.shelf_id)?;
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

/// One of the three rows. Replace does not resolve here WHEN the shelf's
/// copy takes something with it — it moves the sheet to its second ask, which
/// is [`confirm_replace`]'s job. A replace that loses only the row and its
/// name resolves on the spot: the row the reader clicked already said that
/// much, and a second sheet that itemises nothing is a click spent on
/// nothing.
pub fn choose(state: AppState, choice: Choice) {
    if choice == Choice::Replace && replace_warns(state) {
        set_step(state, Step::ConfirmReplace);
        return;
    }
    apply_choice(state, choice);
}

/// One of the same-address question's three rows — [`choose`] for a queue
/// asking that question instead of the file-manager one.
///
/// No second ask belongs here, and that is the point of the question: two rows
/// of ONE address share their highlights and their position, so nothing an
/// answer here does can take something away that the row did not already say.
/// *As new* is the only row that writes a mark of its own
/// ([`library_core::book::Book::independent`]), and the mark is what keeps the
/// two apart from then on.
pub fn choose_linked(state: AppState, choice: LinkedFileChoice) {
    apply_answer(
        state,
        ConflictKind::SameLinkedFile,
        move |state, item| resolve_linked(state, item, choice),
    );
}

/// Whether a Replace owes the second ask: the front of the queue — or, with
/// the switch on, ANY item the batch is about to answer, because one warning
/// covers the whole batch and a loss nobody warned about is a loss the sheet
/// swallowed.
fn replace_warns(state: AppState) -> bool {
    let Some(ask) = state.library.conflict.get_untracked() else {
        return false;
    };
    if ask.apply_all {
        ask.items.iter().any(|item| replace_loses(state, item))
    } else {
        ask.current()
            .is_some_and(|item| replace_loses(state, item))
    }
}

/// What the shelf's copy would take with it, gathered from the live lists:
/// the two rows read from different addresses, and the copy has something at
/// its own — a resume point, or highlights the sweep will take when no row
/// reads the address any more. Two rows of ONE address share their position
/// and their marks (both are keyed by the address, and every writer updates
/// all the rows at it), so the arrival inherits both and the loss is the row
/// and its name, which the first sheet already said.
fn replace_loses(state: AppState, item: &ConflictItem) -> bool {
    let books = state.library.books.get_untracked();
    let Some(existing) = books.iter().find(|b| b.id == item.existing_id) else {
        // The copy went while the sheet was up: there is nothing left to
        // lose, and the resolve below lands the arrival as best it can.
        return false;
    };
    let incoming_path = match &item.placement.incoming {
        Incoming::Move { book_id } => books
            .iter()
            .find(|b| &b.id == book_id)
            .map(|b| b.path().to_string()),
        Incoming::Import { file } => Some(file.path.clone()),
    };
    let positions_differ = incoming_path.is_some_and(|path| path != existing.path());
    let started = existing.page > 1 || existing.fraction.is_some();
    // The gloss is the expensive read (a storage load), and the two cheap
    // facts usually answer without it: same address or never started and
    // nothing marked is nothing to warn about either way.
    let marks = if positions_differ && !started {
        crate::storage::load_gloss()
            .get(&existing.gloss_key())
            .map(Vec::len)
            .unwrap_or(0)
    } else {
        0
    };
    replace_has_losses(positions_differ, started, marks)
}

/// The second ask's rule, as a pure function: the copy has to be leaving an
/// address of its own AND have something there — a position a reader reached
/// or marks a reader made — for the replace to owe a warning.
fn replace_has_losses(positions_differ: bool, started: bool, marks: usize) -> bool {
    positions_differ && (started || marks > 0)
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
    apply_answer(state, ConflictKind::Fingerprint, move |state, item| {
        resolve(state, item, choice)
    });
}

/// The queue's mechanics, shared by both questions: answer the item on screen,
/// then the rest of them when the switch is on, and close the sheet when the
/// queue runs out.
///
/// `kind` is the question `answer` answers, and it is also the guard the
/// switch needs. "The same answer for the rest" is only offered for a queue
/// that asks one kind of question ([`ConflictAsk::uniform`]); this is what
/// makes a queue that somehow holds both kinds safe anyway — the items it can
/// answer are answered and the ones it cannot keep their place on the sheet
/// rather than being handed a word that does not belong to their question.
fn apply_answer(
    state: AppState,
    kind: ConflictKind,
    answer: impl Fn(AppState, &ConflictItem),
) {
    let Some(mut ask) = state.library.conflict.get_untracked() else {
        return;
    };
    if ask.apply_all {
        let mut rest = Vec::new();
        for item in std::mem::take(&mut ask.items) {
            if item.kind == kind {
                answer(state, &item);
            } else {
                rest.push(item);
            }
        }
        if rest.is_empty() {
            cancel(state);
        } else {
            ask.items = rest;
            ask.step = Step::Choose;
            state.library.conflict.set(Some(ask));
        }
        return;
    }
    let Some(item) = ask.items.first().cloned() else {
        cancel(state);
        return;
    };
    ask.items.remove(0);
    answer(state, &item);
    if ask.items.is_empty() {
        cancel(state);
    } else {
        ask.step = Step::Choose;
        state.library.conflict.set(Some(ask));
    }
}

fn resolve(state: AppState, item: &ConflictItem, choice: Choice) {
    match choice {
        Choice::Duplicate => duplicate(state, item, false),
        Choice::Replace => replace(state, item),
        Choice::Merge => merge(state, item),
    }
}

/// The same-address question's three answers.
///
/// Two of them are one row operation wearing two promises, and that is honest
/// rather than lazy: *as new* and *linked* both keep two rows of one file on
/// the shelf, and the difference between them is a single mark on the arrival
/// — the one that decides whether the two go on sharing an address's
/// highlights and resume point or each keep their own. The third writes no row
/// at all.
fn resolve_linked(state: AppState, item: &ConflictItem, choice: LinkedFileChoice) {
    match choice {
        // Nothing to place: the reader asked to be shown the book they already
        // have, which is the library's own reveal — its shelf, then its card.
        LinkedFileChoice::GoToExisting => super::reveal::reveal_book(state, &item.existing_id),
        LinkedFileChoice::AsNew => duplicate(state, item, true),
        LinkedFileChoice::LinkShared => duplicate(state, item, false),
    }
}

// ---------------------------------------------------------------------------
// The three answers.
// ---------------------------------------------------------------------------

/// Keep both: the arrival takes the first free counter name and lands.
///
/// `independent` is the same-address question's *as new* answer and nothing
/// else — the row it makes is the same row either way, and the mark is what
/// makes it a book of its own instead of a twin
/// ([`library_core::book::Book::independent`]). A moved row takes the mark on
/// the row it already is; an imported file takes it on the row this mints.
///
/// The new book's highlight list starts empty, and that is the promise rather
/// than a loss: the marks at the address belong to the book already there, and
/// a book of its own is a book the reader reads separately. What it shares with
/// its twin is the address's fate — the cover, which is the file's art, and a
/// path check, which is a fact about the file.
fn duplicate(state: AppState, item: &ConflictItem, independent: bool) {
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
                    // Added and never taken away: a row that is already a book
                    // of its own stays one, whatever question this sheet is
                    // asking. Clearing the mark would not merely re-share the
                    // row — its highlights live under a key carrying its id, so
                    // a row that stopped being independent would stop reading
                    // the only list it has.
                    row.independent = row.independent || independent;
                }
            });
            land(state, &item.placement, book_id, item.placement.index);
        }
        Incoming::Import { file } => {
            let now = now_ms();
            let name = duplicate_name(state, item);
            let book = Book {
                title: Some(name),
                independent,
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

    let (merged, mut notes) = merge_books(&existing, &incoming);
    // The addresses the fold leaves behind: whichever of the two rows' the
    // merged one does not read from. Their marks and their art travel to the
    // survivor's address first — the union is the point of a merge — through
    // the side-table registry, which counts what it keeps into `notes`, and
    // what is left at them afterwards is swept when no row reads it any more.
    let survivor = merged.path().to_string();
    let mut left_behind: Vec<(String, bool)> = Vec::new();
    for book in [&existing, &incoming] {
        let path = book.path();
        if path != survivor && !left_behind.iter().any(|(p, _)| p == path) {
            left_behind.push((path.to_string(), book.origin.is_stored()));
        }
    }
    // The keys a book of its own reads its marks from, and a fold ends both
    // sides' independence ([`library_core::merge::Policy::Folded`]): the merged
    // row reads the address like every other book, so the marks under an id
    // have to travel there or the union the Merge row promised is a union with
    // a hole in it. The survivor's key goes FIRST, which is what makes
    // `fold_gloss`'s count read the address's own marks as kept rather than as
    // arrived — and the arrival's private marks are its `drop_row`'s to remove
    // afterwards, the sweep that already takes a private row's list with it.
    let mut private_keys: Vec<String> = Vec::new();
    for book in [&existing, &incoming] {
        let key = book.gloss_key();
        if book.independent && key != survivor && !private_keys.contains(&key) {
            private_keys.push(key);
        }
    }

    state.library.books.update(|books| {
        if let Some(row) = books.iter_mut().find(|b| b.id == existing.id) {
            *row = merged;
        }
    });
    for key in &private_keys {
        merge_side::fold_gloss(key, &survivor, &mut notes);
    }
    if existing.independent {
        // The union is at the address now; what is left under the id is the
        // leak `crate::storage::remove_gloss` exists to prevent — the largest
        // half of what a reader put into a book, waiting under a key no row
        // can name any more.
        crate::storage::remove_gloss(&existing.gloss_key());
    }
    for (path, _) in &left_behind {
        merge_side::merge_side_tables(state, path, &survivor, &mut notes);
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
    // The survivor's address may now be one no art was ever rendered for —
    // a fold that healed past a dead address — and the queue is the shelf's
    // one answer to an address with no cover.
    super::covers::backfill_missing(state);
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

/// What the fold would keep — the dry run the sheet's Merge row promises
/// from, the same way the Duplicate row promises the name it would mint.
/// Reads only: no row, no mark and no cover moves here. The notes' resume
/// half comes from [`merge_books`] over the two rows (an import's arrival is
/// the hypothetical row the fold would consume, exactly as [`merge`] builds
/// it), and the marks half from the side tables' own dry counter.
pub fn merge_preview(state: AppState, item: &ConflictItem) -> MergeNotes {
    let Some(existing) = book_by_id(state, &item.existing_id) else {
        return MergeNotes::default();
    };
    let incoming = match &item.placement.incoming {
        Incoming::Move { book_id } => book_by_id(state, book_id),
        Incoming::Import { file } => Some(Book::new(
            String::new(),
            file.fp,
            file.format().unwrap_or(Format::Pdf),
            Origin::Linked {
                src: file.path.clone(),
            },
            now_ms(),
        )),
    };
    let Some(incoming) = incoming else {
        return MergeNotes::default();
    };
    let (merged, mut notes) = merge_books(&existing, &incoming);
    let survivor = merged.path().to_string();
    // Every key the fold gathers marks from, in the order [`merge`] folds
    // them: a private row's id-keyed list first, the survivor's before the
    // arrival's, and then the addresses the fold leaves behind. Same order,
    // same counts — the promise on the sheet is the report the fold writes.
    let mut from: Vec<String> = Vec::new();
    for book in [&existing, &incoming] {
        let key = book.gloss_key();
        if book.independent && key != survivor && !from.contains(&key) {
            from.push(key);
        }
    }
    for book in [&existing, &incoming] {
        let path = book.path().to_string();
        if path != survivor && !from.contains(&path) {
            from.push(path);
        }
    }
    merge_side::count_marks(&from, &survivor, &mut notes);
    notes
}

/// The one line the sheet's Merge row promises: what the fold would keep,
/// counted live from [`merge_preview`] — "4 highlights kept · resumes at
/// page 12 (the further of the two)". A pair with nothing particular to say
/// (two untouched rows, no marks anywhere) gets the general rule instead:
/// the sentence is a promise about THIS pair, and "0 highlights kept" is a
/// promise about nothing.
pub fn merge_note(state: AppState, item: &ConflictItem) -> String {
    let notes = merge_preview(state, item);
    let mut parts: Vec<String> = Vec::new();
    let marks = notes.marks_kept + notes.marks_added;
    if marks > 0 {
        parts.push(format!("{} kept", plural(marks, "highlight", "highlights")));
    }
    if notes.resume_from_incoming {
        parts.push(format!(
            "resumes at page {} (the further of the two)",
            notes.resume_page
        ));
    } else if notes.resume_page > 1 {
        parts.push(format!("resumes at page {}", notes.resume_page));
    }
    if parts.is_empty() {
        return "One book — both sides' highlights, the further position".to_string();
    }
    parts.join(" · ")
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
/// import has always followed. At the ROOT, however, an import lands as its
/// own row: the root's member list is the unfiled rows, the screen has just
/// said none of them holds this content, and a resolve to a shelved twin
/// would land nothing the reader can see. `import::run_files` walks its clean
/// half through here rather than repeating any of it.
pub(super) fn land_clean(state: AppState, placement: &Placement) {
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
            if placement.shelf_id == shelf::ALL_SHELF {
                // Dedupe against the root's own list — the unfiled rows —
                // and nothing else: a shelved twin is WHY this placement was
                // clean, and a twin inside the same batch (one import that
                // measured the same content twice) is the same placement
                // landing twice. `land` has nothing to seat: the books list
                // IS the root, and the push above joined it.
                let shelves = state.library.shelves.get_untracked();
                state.library.books.update(|books| {
                    let held = books
                        .iter()
                        .any(|b| b.fp == book.fp && !is_shelved(&shelves, &b.id));
                    if !held {
                        books.push(book);
                    }
                });
                super::covers::backfill_missing(state);
                return;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::Fingerprint;
    use library_core::shelf::ALL_SHELF;

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn row(id: &str, fingerprint: Fingerprint) -> Book {
        Book::new(
            id.to_string(),
            fingerprint,
            Format::Pdf,
            Origin::Linked {
                src: format!("/books/{id}.pdf"),
            },
            0,
        )
    }

    fn shelf(id: &str, members: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: Default::default(),
            books: members.iter().map(|m| m.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    fn import(fingerprint: Fingerprint, shelf_id: &str) -> Placement {
        Placement {
            incoming: Incoming::Import {
                file: FoundFile {
                    path: "/incoming/new.pdf".to_string(),
                    rel: String::new(),
                    ext: "pdf".to_string(),
                    size: fingerprint.size,
                    fp: fingerprint,
                },
            },
            shelf_id: shelf_id.to_string(),
            from: None,
            index: None,
        }
    }

    fn r#move(book_id: &str, shelf_id: &str, from: Option<&str>) -> Placement {
        Placement {
            incoming: Incoming::Move {
                book_id: book_id.to_string(),
            },
            shelf_id: shelf_id.to_string(),
            from: from.map(str::to_string),
            index: None,
        }
    }

    /// A Markdown row and a Markdown import, for the two tests that run
    /// STATE rather than the pure rule: the cover queue skips anything that
    /// is not a PDF, so a host test that lands a book never starts the wasm
    /// render chain — `cargo test --workspace` runs this crate natively.
    fn md_row(id: &str, path: &str, n: u32) -> Book {
        Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked {
                src: path.to_string(),
            },
            0,
        )
    }

    fn md_import(path: &str, n: u32, shelf_id: &str) -> Placement {
        Placement {
            incoming: Incoming::Import {
                file: FoundFile {
                    rel: path.to_string(),
                    path: path.to_string(),
                    ext: "md".to_string(),
                    size: u64::from(n),
                    fp: fp(n),
                },
            },
            shelf_id: shelf_id.to_string(),
            from: None,
            index: None,
        }
    }

    // -------------------------------------------------------------------
    // The screen's rule: what asks, and what simply lands.
    // -------------------------------------------------------------------

    #[test]
    fn a_move_into_a_shelf_holding_the_same_file_asks() {
        let books = vec![row("b1", fp(1)), row("b2", fp(1))];
        let shelves = vec![shelf("s", &["b1"])];
        // b2 is the same content as b1, which the shelf holds: the drop is a
        // question, and the question names the shelf's copy.
        assert_eq!(
            blocks(&books, &shelves, &r#move("b2", "s", None)).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn an_import_into_the_same_shelf_asks() {
        let books = vec![row("b1", fp(1))];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            blocks(&books, &shelves, &import(fp(1), "s")).as_deref(),
            Some("b1")
        );
        // A shelf that does NOT hold the content lets the same file in: the
        // import resolves to the row the library already has and files that.
        let shelves2 = vec![shelf("s", &["b1"]), shelf("t", &[])];
        assert_eq!(blocks(&books, &shelves2, &import(fp(1), "t")), None);
    }

    #[test]
    fn same_name_different_fp_lands_silently() {
        // Two books that merely rhyme are two books: the match is by
        // CONTENT, never by name — a second format of one title measures a
        // different fingerprint and simply lands.
        let mut twin = row("b2", fp(2));
        twin.title = Some("Dune".to_string());
        let mut held = row("b1", fp(1));
        held.title = Some("Dune".to_string());
        let books = vec![held, twin];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(blocks(&books, &shelves, &r#move("b2", "s", None)), None);
        assert_eq!(blocks(&books, &shelves, &import(fp(2), "s")), None);
    }

    #[test]
    fn reordering_inside_its_own_shelf_never_asks() {
        let books = vec![row("b1", fp(1)), row("b2", fp(1))];
        let shelves = vec![shelf("s", &["b1", "b2"])];
        // The arrival is already the member: a reorder, not a placement —
        // however much of its own content the shelf holds beside it.
        assert_eq!(blocks(&books, &shelves, &r#move("b1", "s", Some("s"))), None);
        // The root's version: an unfiled row dropped back on the root
        // reorders the unfiled list, twin or no twin.
        let root_shelves = vec![shelf("s", &["b2"])]; // b1 is the unfiled one
        assert_eq!(
            blocks(&books, &root_shelves, &r#move("b1", ALL_SHELF, None)),
            None
        );
    }

    #[test]
    fn a_root_import_beside_an_unfiled_twin_asks() {
        // The root level is a target like any other: its member list is the
        // unfiled books. This is the drop that used to vanish — the
        // fingerprint dedupe swallowed it with nothing new on screen.
        let books = vec![row("b1", fp(1))];
        let shelves: Vec<Shelf> = vec![shelf("s", &[])];
        assert_eq!(
            blocks(&books, &shelves, &import(fp(1), ALL_SHELF)).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn a_root_import_of_a_shelved_file_lands_as_its_own_row() {
        // The rule half: the twin is FILED, so it is not a member of the
        // root's own list and the placement screens clean...
        let books = vec![md_row("b1", "/one/dune.md", 1)];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            blocks(&books, &shelves, &md_import("/two/dune.md", 1, ALL_SHELF)),
            None
        );

        // ...and the landing half is what the clean screen promises: the
        // import pushes a row of its own. Resolving it to the shelved twin
        // instead — the dedupe an import into a SHELF follows — would land
        // nothing: the shelf does not change, the root gains no row, and the
        // drop leaves no trace on the screen the reader dropped it on.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(books);
        state.library.shelves.set(shelves);
        land_clean(state, &md_import("/two/dune.md", 1, ALL_SHELF));
        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 2, "the import lands as its own row");
        assert!(rows.iter().any(|b| b.path() == "/two/dune.md" && b.id != "b1"));
        // And a second import of the same content meets the row the first
        // one just made — the unfiled twin that DOES ask.
        let again = md_import("/three/dune.md", 1, ALL_SHELF);
        assert!(blocks(&rows, &state.library.shelves.get_untracked(), &again).is_some());
    }

    #[test]
    fn an_unfiled_book_dropped_on_the_root_never_asks() {
        let books = vec![row("b1", fp(1))];
        let shelves: Vec<Shelf> = Vec::new();
        assert_eq!(blocks(&books, &shelves, &r#move("b1", ALL_SHELF, None)), None);
    }

    #[test]
    fn a_move_to_root_beside_its_unfiled_twin_asks() {
        let books = vec![row("b1", fp(1)), row("b2", fp(1))];
        let shelves = vec![shelf("s", &["b2"])];
        // b2 leaving its shelf for the root lands beside b1, which is the
        // same content and unfiled: the root's list would hold the file
        // twice, so the root asks like a shelf does.
        assert_eq!(
            blocks(&books, &shelves, &r#move("b2", ALL_SHELF, Some("s"))).as_deref(),
            Some("b1")
        );
    }

    // -------------------------------------------------------------------
    // The nest: a folder parked inside another owes the parent a screen.
    // -------------------------------------------------------------------

    #[test]
    fn a_nest_that_parks_a_duplicate_inside_asks() {
        let books = vec![row("b1", fp(1)), row("b2", fp(1))];
        let parent = shelf("p", &["b1"]);
        let mut nested = shelf("f", &["b2", "b1"]);
        nested.parent = Some("p".to_string());
        let shelves = vec![parent, nested];
        // b2 rides the folder into p's view and collides with p's own b1;
        // b1 is already a direct member of the parent, so it screens out as
        // the reorder it is.
        let placements = nest_placements(&shelves, &["f".to_string()], "p");
        assert_eq!(placements.len(), 1);
        assert_eq!(
            blocks(&books, &shelves, &placements[0]).as_deref(),
            Some("b1")
        );
        // A book two of the moved folders both hold is screened once: one
        // arrival, one question.
        let twice = vec![shelf("p", &["b1"]), shelf("f", &["b2"]), shelf("g", &["b2"])];
        assert_eq!(
            nest_placements(&twice, &["f".to_string(), "g".to_string()], "p").len(),
            1
        );
    }

    #[test]
    fn a_nest_without_a_shared_content_screens_clean() {
        let books = vec![row("b1", fp(1)), row("b2", fp(2))];
        let parent = shelf("p", &["b1"]);
        let mut nested = shelf("f", &["b2"]);
        nested.parent = Some("p".to_string());
        let shelves = vec![parent, nested];
        let placements = nest_placements(&shelves, &["f".to_string()], "p");
        assert_eq!(placements.len(), 1);
        assert_eq!(blocks(&books, &shelves, &placements[0]), None);
        // The root is not a nest target: un-filing a folder parks it beside
        // the shelves, and the root's screen is the placement's own.
        assert!(nest_placements(&shelves, &["f".to_string()], ALL_SHELF).is_empty());
    }

    // -------------------------------------------------------------------
    // The same address: the question that is not about content.
    // -------------------------------------------------------------------

    #[test]
    fn the_same_address_asks_the_other_question() {
        // An import of the very file the shelf's row reads: nothing about
        // its content is in doubt, so the three file-manager answers are the
        // wrong three.
        let books = vec![md_row("b1", "/one/dune.md", 1)];
        assert_eq!(
            kind_of(&books, &md_import("/one/dune.md", 1, ALL_SHELF), "b1"),
            ConflictKind::SameLinkedFile
        );
        // A copy of it elsewhere measures the same fingerprint and asks the
        // question the answers were written for: two addresses, two files,
        // one content.
        assert_eq!(
            kind_of(&books, &md_import("/two/dune.md", 1, ALL_SHELF), "b1"),
            ConflictKind::Fingerprint
        );
        // A dragged row is the same question when it reads the same address,
        // and the other one when it does not.
        let twins = vec![md_row("b1", "/one/dune.md", 1), md_row("b2", "/one/dune.md", 1)];
        assert_eq!(
            kind_of(&twins, &r#move("b2", "s", None), "b1"),
            ConflictKind::SameLinkedFile
        );
        let apart = vec![md_row("b1", "/one/dune.md", 1), md_row("b2", "/two/dune.md", 1)];
        assert_eq!(
            kind_of(&apart, &r#move("b2", "s", None), "b1"),
            ConflictKind::Fingerprint
        );
    }

    #[test]
    fn a_row_that_went_is_the_question_that_still_has_answers() {
        // Neither side readable: the generic three, because the answers below
        // resolve a missing row on their own and the sheet has nothing to say
        // about an address it cannot read.
        let books: Vec<Book> = Vec::new();
        assert_eq!(
            kind_of(&books, &md_import("/one/dune.md", 1, ALL_SHELF), "gone"),
            ConflictKind::Fingerprint
        );
        assert_eq!(
            kind_of(&books, &r#move("gone", "s", None), "gone"),
            ConflictKind::Fingerprint
        );
    }

    #[test]
    fn the_sheet_asks_the_question_at_the_front_of_its_queue() {
        let linked = ConflictItem {
            placement: md_import("/one/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::SameLinkedFile,
        };
        let generic = ConflictItem {
            placement: md_import("/two/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::Fingerprint,
        };
        // The flow follows the item on screen rather than the one that opened
        // the sheet: a queue grows by extending whatever sheet is already up.
        let ask = ConflictAsk::new(vec![linked.clone(), generic.clone()]);
        assert!(ask.linked_flow());
        assert!(!ask.uniform(), "two questions, two sets of answers");
        // And the other way round: the front of the queue is the question the
        // sheet asks, whichever kind opened it.
        let ask = ConflictAsk::new(vec![generic.clone(), linked]);
        assert!(!ask.linked_flow());
        assert!(!ask.uniform());
        // One kind all the way down is what the switch needs to mean anything.
        assert!(ConflictAsk::new(vec![generic.clone(), generic]).uniform());
        assert!(!ConflictAsk::new(Vec::new()).uniform(), "no question, no switch");
        assert!(!ConflictAsk::new(Vec::new()).linked_flow());
    }

    #[test]
    fn already_imported_places_nothing_and_lights_the_book_it_names() {
        // The one linked answer a host test can run: it writes no row and
        // touches no storage, so the whole of it is signals.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let books = vec![md_row("b1", "/one/dune.md", 1)];
        state.library.books.set(books.clone());
        state.library.shelves.set(vec![shelf("s", &["b1"])]);
        let item = ConflictItem {
            placement: md_import("/one/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::SameLinkedFile,
        };
        state.library.conflict.set(Some(ConflictAsk::new(vec![item])));
        state.library.conflict_open.set(true);

        choose_linked(state, LinkedFileChoice::GoToExisting);

        assert_eq!(state.library.books.get_untracked(), books, "no row was written");
        assert_eq!(
            state.library.shelf.get_untracked(),
            "s",
            "the breadcrumb moved to the shelf the book is on"
        );
        let (id, first) = state.library.reveal.get_untracked().expect("a reveal");
        assert_eq!(id, "b1");
        assert!(state.library.conflict.get_untracked().is_none(), "the queue ran out");
        assert!(!state.library.conflict_open.get_untracked());

        // Asking again is asking again: the nonce is what makes a second
        // reveal of the same book a second gesture rather than an equal value
        // nobody is told about.
        raise(state, vec![ConflictItem {
            placement: md_import("/one/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::SameLinkedFile,
        }]);
        choose_linked(state, LinkedFileChoice::GoToExisting);
        let (_, second) = state.library.reveal.get_untracked().expect("a second reveal");
        assert_ne!(first, second);
    }

    #[test]
    fn the_switch_answers_the_questions_it_belongs_to() {
        // A queue of both kinds with the switch on: the linked answers resolve
        // the linked items and leave the others on the sheet, because a word
        // from one question is not an answer to the other.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(vec![md_row("b1", "/one/dune.md", 1)]);
        state.library.shelves.set(vec![shelf("s", &["b1"])]);
        let linked = ConflictItem {
            placement: md_import("/one/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::SameLinkedFile,
        };
        let generic = ConflictItem {
            placement: md_import("/two/dune.md", 1, ALL_SHELF),
            existing_id: "b1".to_string(),
            kind: ConflictKind::Fingerprint,
        };
        let mut ask = ConflictAsk::new(vec![linked, generic]);
        ask.apply_all = true;
        state.library.conflict.set(Some(ask));
        state.library.conflict_open.set(true);

        choose_linked(state, LinkedFileChoice::GoToExisting);

        let ask = state.library.conflict.get_untracked().expect("still asking");
        assert_eq!(ask.items.len(), 1, "the linked answer took only the linked question");
        assert_eq!(ask.items[0].kind, ConflictKind::Fingerprint);
        assert_eq!(ask.step, Step::Choose);
        assert!(state.library.reveal.get_untracked().is_some());
    }

    // -------------------------------------------------------------------
    // The second ask: a replace warns only over what it takes.
    // -------------------------------------------------------------------

    #[test]
    fn replace_warns_before_it_writes_and_back_returns_to_choose() {
        // The pure rule first: the copy has to be leaving an address of its
        // own AND have something there.
        assert!(replace_has_losses(true, true, 0), "a resume point goes with the row");
        assert!(replace_has_losses(true, false, 2), "highlights at a swept address go");
        assert!(!replace_has_losses(false, true, 2), "one address shares position and marks");
        assert!(!replace_has_losses(true, false, 0), "only the row and its name — the first sheet said so");

        // Then the wiring: a started book at its own address warns instead
        // of resolving, writes nothing, and Back undoes the detour.
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut existing = md_row("b1", "/one/dune.md", 1);
        existing.page = 240;
        let arrival = md_row("b2", "/two/dune.md", 2);
        let before = vec![existing.clone(), arrival.clone()];
        state.library.books.set(before.clone());
        state.library.shelves.set(vec![shelf("s", &["b1"])]);
        let item = ConflictItem {
            placement: r#move("b2", "s", None),
            existing_id: "b1".to_string(),
            // Two addresses: the file-manager question, and the one whose
            // second ask this test is about.
            kind: ConflictKind::Fingerprint,
        };
        state.library.conflict.set(Some(ConflictAsk::new(vec![item])));
        state.library.conflict_open.set(true);

        choose(state, Choice::Replace);
        let ask = state.library.conflict.get_untracked().unwrap();
        assert_eq!(ask.step, Step::ConfirmReplace, "the warning stands before the write");
        assert_eq!(ask.items.len(), 1, "nothing resolved while it stands");
        assert_eq!(state.library.books.get_untracked(), before, "and nothing was written");

        back_to_choices(state);
        let ask = state.library.conflict.get_untracked().unwrap();
        assert_eq!(ask.step, Step::Choose, "Back is the three answers again");
        assert_eq!(ask.items.len(), 1);
    }
}
