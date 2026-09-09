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

use super::effect::{DropEffect, DropQuery, FoldPreview, drop_effect, fold_items, fold_preview};
use super::target::{DropTargetId, DropTargetKind, DropTargetRegistry};
use super::{FOLD_DWELL_MS, commit};
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
            dwell: RwSignal::new(false),
            dwell_target: RwSignal::new(None),
            registry: DropTargetRegistry::new(),
            state,
        };
        provide_context(this);
        this.bind_session();
        this.bind_dwell();
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

    /// The fold's dwell: one timer, owned by an effect on the target it counts
    /// against, so moving to another card clears it and so does the drag ending.
    ///
    /// Only a book brews a fold. A folder under the pointer is already a shelf,
    /// and offering to make a second one out of what is held and the first would
    /// be an offer with nothing in it.
    fn bind_dwell(&self) {
        let this = *self;
        Effect::new(move |_| {
            let Some(target) = this.dwell_target.get() else {
                return;
            };
            if target.0 != DropTargetKind::Book {
                return;
            }
            let Ok(handle) = set_timeout_with_handle(
                move || {
                    this.dwell.set(true);
                    this.refresh();
                },
                Duration::from_millis(FOLD_DWELL_MS as u64),
            ) else {
                return;
            };
            on_cleanup(move || handle.clear());
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

    /// Whether a release over this book would land the held items before it.
    pub fn inserts_before(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.insert_before()) == Some(id))
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

    /// Whether the pointer is over the crumb for `id`. An empty id is the root's
    /// `All`, which is a crumb and not a shelf.
    pub fn over_shelf(&self, id: &str) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Shelf && target.1 == id)
        })
    }

    // -- the session's own arithmetic ---------------------------------------

    /// Move the pointer, and with it the answer to "what would a release mean".
    fn on_move(&self, x: f64, y: f64) {
        self.pointer.set((x, y));
        let next = self.registry.hit_test(x, y);
        if next == self.hot.get_untracked() {
            return;
        }
        self.hot.set(next.clone());
        // A new target starts its dwell from nothing, and the fold that was
        // brewing over the last one goes with it.
        self.dwell.set(false);
        self.dwell_target.set(next);
        self.refresh();
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
            dwell_armed: self.dwell.get_untracked(),
        };
        let effect = drop_effect(query);
        self.fold.set(match &effect {
            DropEffect::CreateFolder { with_book_id } => {
                fold_preview(fold_items(&query), with_book_id)
            }
            _ => None,
        });
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
        self.clear_answer();
        if applied && self.state.library.selecting.get_untracked() {
            exit_selection(self.state);
        }
    }
}
