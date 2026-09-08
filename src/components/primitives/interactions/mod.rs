//! Interaction primitives: the gesture mechanics a card or a mark layer spreads
//! onto an element. The window-listener wiring lives here once; domain
//! components keep the policy (what a drag writes, what a long press means).
//!
//!   * [`long_press`] — one gesture: a hold, its slop, and the suppression of
//!     the click that follows it. What a stroke on a page answers to.
//!   * [`draggable_item`] — the three gestures a shelf card answers to, decided
//!     once per press so they cannot race. Built on the same hold tuning.
//!   * [`drag`] — a raw pointer stream on `window`, for the surfaces that move
//!     something themselves.

pub mod drag;
pub mod draggable_item;
pub mod long_press;
