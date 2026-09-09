//! Library drag & drop: one pointer-driven session for every move a reader makes
//! on the shelf.
//!
//! Nothing in here rides the browser's own drag-and-drop. Three things decided
//! that, and the first is a bug a reader can see: once the engine takes a drag
//! over, `pointerup` never reaches the element the press began on, so the card's
//! own "I am being held" flag had no release to clear it and the card stayed
//! faded until a click somewhere else dismissed the selection it had turned on.
//! The other two are things a browser drag cannot do at any price — it will not
//! say how LONG a drag has hovered a target, which is the whole of the fold
//! gesture, and it will not draw a ghost of the four covers a reader is holding,
//! because its image is one bitmap of the one element the press started on.
//!
//! So the shelf's moves ride pointer events end to end:
//!
//!   * [`controller`] — the session: what is held, where the pointer is, which
//!     target is hot, whether a fold is brewing, and the one place a drag ends.
//!   * [`target`] — the registry of things a drop can land on. A card joins it
//!     with one call and leaves it when it unmounts.
//!   * [`effect`] — the decision table, pure and unit-tested: (what is held, what
//!     is under the pointer, how long it has been there) → what a release means.
//!   * [`commit`] — the only place a decision touches library state, through the
//!     services a menu-driven move uses.
//!   * [`layer`] — the overlay that carries the held covers and the fold preview.
//!
//! The press itself stays with
//! `crate::components::primitives::interactions::draggable_item`, which is what
//! tells a tap from a hold from a movement. This module starts where that
//! decision lands on "movement", and the window's file-drop machinery in
//! `crate::effects::app::drag_drop` keeps the drags that arrive from OUTSIDE —
//! the two never meet, because a pointer drag raises no DOM `dragstart` for the
//! window to stand aside from.

pub mod commit;
pub mod controller;
pub mod effect;
pub mod layer;
pub mod target;

/// How long the pointer must rest over one book before the drag offers to fold
/// everything held — and that book — into a new shelf.
///
/// Longer than the hold that starts a selection
/// (`crate::components::primitives::interactions::long_press::SELECT_PRESS_MS`),
/// on purpose: a reader crossing a shelf on the way to somewhere else rests over
/// cards, and a fold that armed at the hold's tuning would offer a new shelf on
/// every drag that happened to slow down over a second book.
pub const FOLD_DWELL_MS: i32 = 650;
