//! The drag session: what is held, where the pointer is, which target is hot,
//! and what a release right now would mean.
//!
//! One controller per library page, provided by
//! `crate::features::library::page` and read by every card, row and crumb under
//! it. That is the fix for the shelf's stuck drag, and it is structural rather
//! than a better-reasoned flag: the session lives in one place, it ends on
//! `pointerup`, on `pointercancel` and on Escape, and every one of those three
//! goes through the same [`DragController::end`] — so there is no sequence of
//! events that leaves a card believing it is still being held.
//!
//! The press that starts a session is still decided by
//! `crate::components::primitives::interactions::draggable_item`, which is what
//! tells a tap from a hold from a movement. A card hands the movement here and
//! keeps the other two.
//!
//! The listeners belong to the SESSION and not to the card: they are on `window`
//! for as long as a drag is live. A card that unmounts mid-drag — a focus rescan
//! filing a book somewhere else while the reader is holding it — takes its own
//! pointer handlers with it, and a drag whose release only that card could hear
//! is a drag that never ends.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use library_core::book::Book;
use library_core::shelf::{ALL_SHELF, Shelf, can_nest};

use super::effect::{
    Band, DropEffect, DropQuery, FoldPreview, drop_effect, fold_items, fold_preview,
};
use super::target::{DropTargetId, DropTargetKind, DropTargetRegistry};
use super::{FOLD_DWELL_MS, SINK_DWELL_MS, commit};
use crate::features::library::selection::exit_selection;
use crate::state::AppState;

/// What one press picked up.
///
/// One struct rather than a book-or-folder enum, because a selection holds both
/// and a reader who held three books and a shelf meant all four: every move the
/// library can make is a pair of operations on one shelf list — the books become
/// members and the folders are nested — so a payload that arrived pre-split is a
/// payload the commit step does not have to sort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DragPayload {
    pub books: Vec<String>,
    pub folders: Vec<String>,
}

impl DragPayload {
    pub fn len(&self) -> usize {
        self.books.len() + self.folders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this item is one of the held ones. Asked by every card on every
    /// repaint of a drag, which is the reason it is a question about the payload
    /// rather than a derived set of its own.
    pub fn contains(&self, id: &str) -> bool {
        self.books.iter().any(|each| each.as_str() == id)
            || self.folders.iter().any(|each| each.as_str() == id)
    }
}

/// One tile of the ghost the drag layer carries: a cover when the library has
/// art for the item, and the item's name when it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostTile {
    pub cover: Option<String>,
    /// The item's name — the tile's tooltip, and the letter a tile with no cover
    /// shows.
    pub label: String,
    /// Whether the item is a shelf rather than a book. Drawn as a folder rather
    /// than as a letter, because a shelf has no cover to stand in for.
    pub folder: bool,
}

/// Where a sunk ghost sits: the centre of the target it is sinking into, in
/// viewport coordinates.
///
/// Captured once, when the dwell runs out, rather than read per frame. A sunk
/// ghost is a promise that the pointer has stopped moving — the reader is
/// deciding, not dragging — so the target's box is not changing either, and a
/// layout read on every animation frame would be a read nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkSpot {
    pub x: f64,
    pub y: f64,
}

/// The library page's drag session.
#[derive(Clone, Copy)]
pub struct DragController {
    /// Whether a drag is live. The effect that binds the window's pointer
    /// listeners reads this, so those listeners exist exactly while a drag does
    /// and not for the life of the page.
    session: RwSignal<bool>,
    payload: RwSignal<Option<DragPayload>>,
    /// The pointer's client coordinates, which is where the layer draws.
    pointer: RwSignal<(f64, f64)>,
    /// The topmost target under the pointer.
    hot: RwSignal<Option<DropTargetId>>,
    /// What a release right now would do. Written by [`DragController::refresh`]
    /// rather than derived, because the only things that can change it — the hot
    /// target, the payload, the dwell — are all written here too, and a card that
    /// painted itself from a memo of a memo would be a card two frames behind the
    /// pointer.
    effect: RwSignal<Option<DropEffect>>,
    /// The fold brewing under the pointer, if one is.
    fold: RwSignal<Option<FoldPreview>>,
    /// Where the ghost has sunk to, while it has. `None` is the ghost following
    /// the pointer, which is where it lives most of a drag — and the one fact the
    /// layer's whole appearance is derived from, its transition included, so there
    /// is no second signal free to disagree about whether the ghost is parked or
    /// following.
    sink: RwSignal<Option<SinkSpot>>,
    /// The sunk target's box as `(left, top, right, bottom)`, cached when the sink
    /// armed. While parked this is the ONLY thing a pointermove is tested against:
    /// no DOM measurement, no signal write, no re-render, until the pointer leaves
    /// the target it is sunk in. A held pointer on a crumb costs one comparison
    /// per move instead of a `getBoundingClientRect` for every target on the shelf.
    ///
    /// A cache and not a live read, which makes it stale under a scroll — the same
    /// promise [`SinkSpot`] already makes. A sunk ghost says the pointer has
    /// stopped; a reader scrolling the shelf under a parked drag is asking
    /// something else, and the first move out of the cached box resumes the follow.
    sink_rect: RwSignal<Option<(f64, f64, f64, f64)>>,
    /// Whether the dwell over the hot target has run out.
    dwell: RwSignal<bool>,
    /// The target the dwell is counting against. Separate from [`Self::hot`]
    /// because the dwell's timer is an effect on this signal: moving to another
    /// card re-runs it, and its cleanup is what clears the timer that was counting.
    dwell_target: RwSignal<Option<DropTargetId>>,
    /// The targets on the page. Public because a card joins it, and the only part
    /// of a session anything outside this module writes.
    pub registry: DropTargetRegistry,
    state: AppState,
}

impl DragController {
    /// Install the session for a library page and provide it to everything under
    /// it. Called once, by the page, before the content that reads it.
    pub fn install(state: AppState) -> Self {
        let this = Self {
            session: RwSignal::new(false),
            payload: RwSignal::new(None),
            pointer: RwSignal::new((0.0, 0.0)),
            hot: RwSignal::new(None),
            effect: RwSignal::new(None),
            fold: RwSignal::new(None),
            sink: RwSignal::new(None),
            sink_rect: RwSignal::new(None),
            dwell: RwSignal::new(false),
            dwell_target: RwSignal::new(None),
            registry: DropTargetRegistry::new(),
            state,
        };
        provide_context(this);
        this.bind_session();
        this.bind_dwells();
        this.bind_escape();
        this
    }

    // -- the session's wiring ----------------------------------------------

    /// The window's pointer stream, alive exactly while a drag is.
    ///
    /// Not `crate::components::primitives::interactions::drag`: that primitive
    /// finishes a drag one way, and here a release and a cancellation are
    /// different answers — one commits what the reader was holding and the other
    /// puts it back.
    fn bind_session(&self) {
        let this = *self;
        Effect::new(move |_| {
            if !this.session.get() {
                return;
            }
            let moved = window_event_listener_untyped("pointermove", move |ev: web_sys::Event| {
                let at = ev.unchecked_ref::<web_sys::MouseEvent>();
                this.on_move(at.client_x() as f64, at.client_y() as f64);
            });
            let released = window_event_listener_untyped("pointerup", move |ev: web_sys::Event| {
                let at = ev.unchecked_ref::<web_sys::MouseEvent>();
                this.release(at.client_x() as f64, at.client_y() as f64);
            });
            let taken = window_event_listener_untyped("pointercancel", move |_| this.cancel());
            on_cleanup(move || {
                moved.remove();
                released.remove();
                taken.remove();
            });
        });
    }

    /// The two dwells: one timer each, both owned by an effect on the target they
    /// count against, so moving to another target clears them and so does the drag
    /// ending.
    ///
    /// They are not one question at two depths, and the difference is the point.
    /// The sink belongs to the TITLE BAR alone: a crumb is the one target on the
    /// page smaller than the ghost that hovers it, so a ghost at full size over a
    /// crumb covers the only thing the reader is aiming at — the name of the level
    /// the held items are about to go to. Shrinking to a third of its size on the
    /// crumb's centre is what leaves that name sticking out on both sides, and it
    /// is a picture of the drop, because a crumb IS the place things go into.
    ///
    /// Nothing on the shelf itself sinks. A folder card does not need it: it is
    /// nine and a half remits across and it already wears the loudest marker in
    /// the shelf's vocabulary — the accent ring, the halo and the plate lifting
    /// (`folder-drag-over`) — so a shrink on top of that is a second, slower answer
    /// to a question the ring answered on the frame the pointer arrived, and it
    /// takes the covers away from a reader at the moment they are checking what
    /// they are holding. A book is not a container at all: it is a position, which
    /// the insertion line beside it already draws, or a fold partner, which the
    /// plate draws instead of the ghost. And the level's empty space has a box the
    /// size of the scroll container, so its centre is the middle of the screen —
    /// sinking there is the ghost leaving the reader's hand for a place they are
    /// not pointing at.
    fn bind_dwells(&self) {
        let this = *self;
        Effect::new(move |_| {
            let Some(target) = this.dwell_target.get() else {
                return;
            };
            // A crumb that would refuse the drag wears no highlight, so it must
            // not wear a ghost either: the two are the same promise. (No crumb
            // refuses today — filing onto a level accepts anything — but the sink
            // reads the answer rather than assuming it, so a refusal added to the
            // table is a refusal the ghost honours without anybody remembering.)
            let sinkable = target.0 == DropTargetKind::Shelf
                && this.effect.get_untracked().as_ref() != Some(&DropEffect::Refused);
            if sinkable
                && let Some(rect) = this.registry.rect_of(&target)
            {
                let spot = SinkSpot {
                    x: rect.left() + rect.width() / 2.0,
                    y: rect.top() + rect.height() / 2.0,
                };
                let bounds = (rect.left(), rect.top(), rect.right(), rect.bottom());
                let still = target.clone();
                // A timer that would not schedule costs the sink and nothing else,
                // so the fold below is armed either way.
                if let Ok(sunk) = set_timeout_with_handle(
                    move || {
                        // Still the same target: the effect's own cleanup clears
                        // this timer when the target changes, and a timer that
                        // fired anyway would sink the ghost into a box the pointer
                        // has already left.
                        if this.dwell_target.get_untracked().as_ref() != Some(&still) {
                            return;
                        }
                        this.sink.set(Some(spot));
                        this.sink_rect.set(Some(bounds));
                    },
                    Duration::from_millis(SINK_DWELL_MS as u64),
                ) {
                    on_cleanup(move || sunk.clear());
                }
            }
            // The fold's dwell is armed off the ANSWER and not only off the
            // target's kind: a book the session currently reads as a landing —
            // a position (`InsertBefore`), or the folders-only filing that
            // answers a hold with no books in it — is a book a rest can turn
            // into a partner. The answer is the session's own truth about the
            // pointer — it has already collapsed which node of the row the
            // hit-test landed on — so a rest over a book works even when the
            // hit-test resolved through one of the row's children, and a
            // target with no live answer arms no timer at all.
            let arms_fold = matches!(
                this.effect.get_untracked(),
                Some(DropEffect::InsertBefore { .. }) | Some(DropEffect::FileToShelf { .. })
            ) && target.0 == DropTargetKind::Book;
            if !arms_fold {
                return;
            }
            let still = target;
            let Ok(folded) = set_timeout_with_handle(
                move || {
                    if this.dwell_target.get_untracked().as_ref() != Some(&still) {
                        return;
                    }
                    this.dwell.set(true);
                    this.refresh();
                },
                Duration::from_millis(FOLD_DWELL_MS as u64),
            ) else {
                return;
            };
            on_cleanup(move || folded.clear());
        });
    }

    /// Escape puts the held items back.
    ///
    /// One listener for the page rather than one per session, because a session
    /// that has not started has nothing to cancel and [`DragController::cancel`]
    /// says so in its first line.
    fn bind_escape(&self) {
        let this = *self;
        let handle = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            if let Ok(key) = ev.dyn_into::<web_sys::KeyboardEvent>()
                && key.key() == "Escape"
            {
                this.cancel();
            }
        });
        on_cleanup(move || handle.remove());
    }

    // -- what a card calls --------------------------------------------------

    /// Pick `payload` up at the pointer. Called from the card wrapper's
    /// drag-start, which is the moment a press has been decided as a movement
    /// rather than as a tap or a hold.
    pub fn begin(&self, payload: DragPayload, x: f64, y: f64) {
        // One drag at a time, and nothing at all for a press that picked up
        // nothing: a card that is not in the library any more has no payload and
        // no business starting a session.
        if payload.is_empty() || self.session.get_untracked() {
            return;
        }
        self.payload.set(Some(payload));
        self.pointer.set((x, y));
        // The ghost arrives at the pointer before the first move does, and with
        // no target under it: a drop line on the card being lifted is a line
        // offering the place the reader has not asked for yet.
        self.session.set(true);
    }

    /// Put the held items down where the pointer is.
    pub fn release(&self, x: f64, y: f64) {
        if !self.session.get_untracked() {
            return;
        }
        self.pointer.set((x, y));
        // One last hit at the release point rather than the last reported move:
        // a fast drag ends with the pointer somewhere the stream never sampled,
        // and where the reader let go is the answer they meant.
        let next = self.registry.hit_test(x, y);
        if next != self.hot.get_untracked() {
            self.hot.set(next);
        }
        self.refresh();
        let effect = self.effect.get_untracked();
        // A release over nothing, and a release over a target that refuses, are
        // both a drag that did not happen — and a drag that did not happen must
        // not consume the selection the reader was holding.
        let applied = effect
            .as_ref()
            .is_some_and(|each| *each != DropEffect::Refused);
        if let Some(effect) = effect {
            let payload = self.payload.get_untracked().unwrap_or_default();
            commit::apply(self.state, effect, payload);
        }
        self.end(applied);
    }

    /// Put the held items back. A cancellation, an Escape, and the page going
    /// away all arrive here.
    pub fn cancel(&self) {
        if !self.session.get_untracked() {
            return;
        }
        self.end(false);
    }

    // -- what a card paints itself from -------------------------------------

    /// Whether a drag is live at all. The layer's own visibility.
    pub fn live(&self) -> Signal<bool> {
        self.session.into()
    }

    /// Where the layer draws.
    pub fn pointer(&self) -> Signal<(f64, f64)> {
        self.pointer.into()
    }

    /// The fold brewing right now, if one is.
    pub fn fold(&self) -> Signal<Option<FoldPreview>> {
        self.fold.into()
    }

    /// Where the ghost has sunk to, or `None` while it follows the pointer.
    pub fn sink(&self) -> Signal<Option<SinkSpot>> {
        self.sink.into()
    }

    /// Whether `id` is one of the items being held. What every held card fades
    /// on, which is the visible half of a multi-drag: the set the reader picked
    /// up stays readable as a set while the pointer carries it.
    pub fn holds(&self, id: &str) -> bool {
        self.payload
            .with(|at| at.as_ref().is_some_and(|held| held.contains(id)))
    }

    /// How many items are held.
    pub fn count(&self) -> Signal<usize> {
        let this = *self;
        Signal::derive(move || this.payload.get().map(|held| held.len()).unwrap_or(0))
    }

    /// The ghost's tiles, in payload order: books first, because a shelf's plate
    /// is made of the covers beside it and not the other way round.
    pub fn ghost(&self) -> Signal<Vec<GhostTile>> {
        let this = *self;
        Signal::derive(move || {
            let Some(held) = this.payload.get() else {
                return Vec::new();
            };
            let state = this.state;
            let books: Vec<Book> = state.library.books.get();
            let shelves: Vec<Shelf> = state.library.shelves.get();
            let covers = state.library.covers.get();
            let mut tiles = Vec::with_capacity(held.len());
            for id in &held.books {
                let Some(book) = books.iter().find(|each| &each.id == id) else {
                    continue;
                };
                tiles.push(GhostTile {
                    cover: covers.get(book.path()).map(|cover| cover.data_url.clone()),
                    label: book.title(),
                    folder: false,
                });
            }
            for id in &held.folders {
                tiles.push(GhostTile {
                    cover: None,
                    label: shelves
                        .iter()
                        .find(|each| &each.id == id)
                        .map(|each| each.name.clone())
                        .unwrap_or_default(),
                    folder: true,
                });
            }
            tiles
        })
    }

    /// Whether a release over this book would land the held items BEFORE it —
    /// the top seam of its row, and the only seam the grid's cards have.
    pub fn inserts_before(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.insert_at()) == Some((id, false)))
    }

    /// Whether a release over this book would land the held items AFTER it —
    /// the bottom seam of a list row, which is the same position seen from the
    /// other side of the row that names it.
    pub fn inserts_after(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.insert_at()) == Some((id, true)))
    }

    /// Whether a release over this shelf row would reorder the held folders
    /// beside it, and on which side. `None` for every other answer, which is
    /// what a row paints its sibling seams from.
    pub fn sibling_at(&self, id: &str) -> Option<bool> {
        self.effect.with(|at| {
            at.as_ref()
                .and_then(|each| each.sibling_at())
                .and_then(|(anchor, after)| (anchor == id).then_some(after))
        })
    }

    /// Whether this book is the one a fold is brewing over.
    pub fn folds_with(&self, id: &str) -> bool {
        self.fold.with(|at| {
            at.as_ref()
                .is_some_and(|preview| preview.with_book_id == id)
        })
    }

    /// Whether a release over this folder would file the held items in it. False
    /// for a folder that refuses the drag, which is the point: a ring on a drop
    /// that will not happen is a promise the commit step breaks.
    pub fn nests_into(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.nest_into()) == Some(id))
    }

    /// Whether the pointer is over the bar's ellipsis — the folded levels'
    /// affordance, which is a place to rest and not a place to drop.
    ///
    /// Asked by the breadcrumb while a drag is live, because a drag cannot raise
    /// a `mouseenter`: the card the press began on holds the pointer capture, and
    /// a captured pointer reports its boundary events to the capture target
    /// alone. The registry's geometry is the one thing under a capture that still
    /// tells the truth, so the fold opens from this instead.
    pub fn over_ellipsis(&self) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Ellipsis)
        })
    }

    /// Whether the pointer is over the crumb for `id`. An empty id is the root's
    /// `All`, which is a crumb and not a shelf.
    pub fn over_shelf(&self, id: &str) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Shelf && target.1 == id)
        })
    }

    /// Whether the pointer is over the shelf row (or folder card) for `id`.
    ///
    /// The tree's hover-to-expand asks it: a hold resting on a COLLAPSED shelf
    /// row is the courtesy every file manager's tree gives a drag — the way
    /// deeper is the way in, and the reader should not have to put the hold
    /// down to knock. It is a question about the hot target rather than about
    /// the effect, because the answer is the same at every band of the row.
    pub fn over_folder(&self, id: &str) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Folder && target.1 == id)
        })
    }

    // -- the session's own arithmetic ---------------------------------------

    /// Move the pointer, and with it the answer to "what would a release mean".
    fn on_move(&self, x: f64, y: f64) {
        // Parked on a crumb: the ghost is reading the target, not the hand.
        // One cached rect test and a return — no measurement, no signal write, no
        // re-render — so a held pointer on a crumb costs nothing per move. And
        // the first move that LEAVES the box is the first one the follow resumes
        // on: the sink lifts, the layer's transition lifts with it because the two
        // are the same signal, and the ghost is back under the cursor with nothing
        // trailing behind it.
        if let Some((left, top, right, bottom)) = self.sink_rect.get_untracked() {
            if x >= left && x <= right && y >= top && y <= bottom {
                return;
            }
            // Out of the box that was cached, so follow the hand again even when
            // the target underneath turns out to be the same one. A cached box can
            // go stale without the target changing — a wheel scroll mid-drag, or a
            // level re-laying itself out under an import — and a stale box is not
            // a place to stay parked. What is NOT worth doing here is re-arming the
            // dwell on a target the pointer never left: the drop itself is decided
            // by the fresh hit-test in [`DragController::release`], so a ghost that
            // follows the hand instead of re-parking costs a picture and never a
            // move.
            self.release_sink();
        }
        self.pointer.set((x, y));
        let next = self.registry.hit_test(x, y);
        if next == self.hot.get_untracked() {
            return;
        }
        self.hot.set(next.clone());
        // A new target starts both dwells from nothing, and the fold that was
        // brewing over the last one goes with it. The sink and the box it cached go
        // too: a ghost left sunk in a box the pointer has abandoned is a ghost
        // parked in mid-air.
        self.dwell.set(false);
        self.release_sink();
        self.dwell_target.set(next);
        self.refresh();
    }

    /// Lift the sink, in one place. The spot and the box it was cached from are one
    /// fact, and clearing them apart would leave the fast path above testing a box
    /// that nothing is sunk in — which is a drag that stops following the pointer.
    fn release_sink(&self) {
        self.sink.set(None);
        self.sink_rect.set(None);
    }

    /// Re-answer "what would a release mean" from where the pointer is now.
    ///
    /// Every read is untracked: this runs from a pointer event and a timer, not
    /// from a reactive scope, and the answer is written to [`Self::effect`] for
    /// the cards to subscribe to. A tracked read here would subscribe whatever
    /// scope happened to be current to the whole library.
    fn refresh(&self) {
        let Some(held) = self.payload.get_untracked() else {
            return self.clear_answer();
        };
        let Some(target) = self.hot.get_untracked() else {
            return self.clear_answer();
        };
        // The level's empty space is the shelf the page is on, and the root has
        // no shelf — which the table spells as an empty id and the commit step
        // reads as "take them off whatever holds them".
        let open = self.state.library.shelf.get_untracked();
        // The container the row answers to: the entry's own shelf when the row
        // named one — a book inside an expanded tree belongs to THAT shelf, not
        // to the level the page is on — and the open level otherwise, which is
        // the container a grid card and a flat row both imply. `None` at the
        // root, where the library's own order is the member list.
        let row_shelf = match self.registry.entry_of(&target).and_then(|each| each.shelf) {
            Some(named) => (named != ALL_SHELF).then_some(named),
            None => (open != ALL_SHELF).then(|| open.clone()),
        };
        let target_id = match target.0 {
            DropTargetKind::Level if open == ALL_SHELF => String::new(),
            DropTargetKind::Level => open,
            _ => target.1.clone(),
        };
        let query = DropQuery {
            held_books: held.books.len(),
            held_folders: held.folders.len(),
            target_kind: target.0,
            target_id: &target_id,
            target_is_held: held.contains(&target.1),
            can_nest: target.0 == DropTargetKind::Folder && self.can_nest_held(&held, &target.1),
            can_sibling: target.0 == DropTargetKind::Folder
                && self.can_sibling_held(&held, &target.1),
            band: self.band_of(&target),
            target_shelf: row_shelf.as_deref(),
            dwell_armed: self.dwell.get_untracked(),
        };
        let effect = drop_effect(query);
        self.fold.set(match &effect {
            DropEffect::CreateFolder { with_book_id } => {
                fold_preview(fold_items(&query), with_book_id)
            }
            _ => None,
        });
        // A brewing fold outranks the sink. The two are the same gesture at two
        // depths — "it lands here", then "it becomes a shelf here" — and the
        // second one is drawn as a plate the reader has to be able to read: a
        // plate shrunk to a third of itself inside the card it is offering to
        // replace is a plate that says nothing.
        if self.fold.get_untracked().is_some() {
            self.release_sink();
        }
        self.effect.set(Some(effect));
    }

    fn clear_answer(&self) {
        self.effect.set(None);
        self.fold.set(None);
    }

    /// Whether every held shelf may be filed inside `target`.
    ///
    /// Asked per held shelf rather than for the batch, because
    /// `library_core::shelf::can_nest` is a question about one edge of the tree
    /// and a batch answer of "no" would refuse a drag that could have filed two
    /// of its three folders.
    fn can_nest_held(&self, held: &DragPayload, target: &str) -> bool {
        self.state.library.shelves.with_untracked(|shelves| {
            held.folders
                .iter()
                .all(|each| can_nest(shelves, each, target))
        })
    }

    /// Whether every held shelf may sit beside `anchor` as a sibling.
    ///
    /// The graph question is the PARENT's: filing beside a shelf is filing into
    /// the level that holds it, so `can_nest` is asked of that level — and a
    /// root-level seam has no parent to close a loop through, which is why an
    /// anchor at the top of the library only refuses a shelf asked to sibling
    /// itself.
    fn can_sibling_held(&self, held: &DragPayload, anchor: &str) -> bool {
        self.state.library.shelves.with_untracked(|shelves| {
            let Some(target) = shelves.iter().find(|s| s.id == anchor) else {
                return false;
            };
            held.folders.iter().all(|each| {
                each != anchor
                    && target
                        .parent
                        .as_deref()
                        .is_none_or(|parent| can_nest(shelves, each, parent))
            })
        })
    }

    /// Which part of the target's box the pointer is on: the list's seams, and
    /// the grid's non-question — outside the list layout every target is its
    /// whole self and the answer is the middle.
    ///
    /// Computed here rather than in the table or the rows because three things
    /// have to agree about it — the seam a row paints, the effect the table
    /// answers, and the index the commit resolves — and one rectangle read in
    /// one place is the only way they cannot drift.
    fn band_of(&self, target: &DropTargetId) -> Band {
        if !self.state.library.view.with_untracked(|v| v.is_list()) {
            return Band::Middle;
        }
        let Some(rect) = self.registry.rect_of(target) else {
            return Band::Middle;
        };
        let (_, y) = self.pointer.get_untracked();
        let at = (y - rect.top()) / rect.height().max(1.0);
        match target.0 {
            // A shelf row is a container first and a seam second: only its
            // outer quarters reorder, and its middle half stays the mouth.
            DropTargetKind::Folder => {
                if at < 0.25 {
                    Band::Top
                } else if at > 0.75 {
                    Band::Bottom
                } else {
                    Band::Middle
                }
            }
            // A book row is two positions and nothing else.
            _ => {
                if at < 0.5 {
                    Band::Top
                } else {
                    Band::Bottom
                }
            }
        }
    }

    /// End the session, however it ended.
    ///
    /// The one place a drag stops being a drag: the listeners go with the effect
    /// that owns them, the dwell's timer goes with the effect that owns it, and
    /// everything the cards paint themselves from is written back to nothing in
    /// the same breath. There is no path out of a drag that does not come through
    /// here, which is what makes a stuck "still being held" card unrepresentable
    /// rather than merely unlikely.
    ///
    /// `applied` is whether the drop did something, and it decides the selection's
    /// fate: a move consumes the set the way every other action on this shelf does
    /// — the bar's own rows all leave the reader holding nothing afterwards — while
    /// a drag that was cancelled, or released over nothing, leaves a reader who was
    /// choosing still choosing.
    fn end(&self, applied: bool) {
        self.session.set(false);
        self.payload.set(None);
        self.pointer.set((0.0, 0.0));
        self.hot.set(None);
        self.dwell.set(false);
        self.dwell_target.set(None);
        self.release_sink();
        self.clear_answer();
        if applied && self.state.library.selecting.get_untracked() {
            exit_selection(self.state);
        }
    }
}
