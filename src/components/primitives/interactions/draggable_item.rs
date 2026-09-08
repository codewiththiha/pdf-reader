//! One gesture wrapper for a card: tap, hold and drag, decided once.
//!
//! A shelf card answers to three pointers at once — a tap opens, a hold starts
//! a multi-select, a movement files the book somewhere else — and the three
//! arrive as the same `pointerdown`. Handled separately (a click listener, a
//! long-press primitive, the browser's own drag) they race: the hold completes
//! and its exhaust click opens the book it was meant to select, or the press
//! drifts two pixels and cancels a gesture the reader was still making.
//!
//! So the mode is decided ONCE and locked until the pointer is released. The
//! first of these to happen wins and the other two are then unreachable:
//!
//!   * the hold timer fires → [`Mode::Hold`], and the click and the synthetic
//!     contextmenu that follow it are swallowed;
//!   * the pointer travels past [`DRAG_THRESHOLD_PX`] → [`Mode::Drag`] when the
//!     caller allows a drag, [`Mode::Abandoned`] when it does not;
//!   * the pointer is released having done neither → [`Mode::Tap`].
//!
//! Travelling past the threshold while nothing is draggable is its own answer
//! rather than a tap: on a touch surface that movement is a scroll, and opening
//! the book the reader was scrolling past is exactly the surprise this module
//! exists to prevent.
//!
//! The drag this decides is the POINTER half only. Filing a book somewhere
//! still travels over the browser's own drag-and-drop — one `DataTransfer`, the
//! window's file-drop overlay already told apart from it, and a drop index the
//! target reads out of the order it rendered. What this adds is the decision
//! that a movement means "drag" and not "hold", made before the browser makes
//! it for us. See `crate::features::library::drag` for the payload half.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// What a press has to have begun on for the card to stand aside.
///
/// A card holds controls of its own — the ✕ that asks for a removal receipt, the
/// relink that finds a book again — and their `click` handlers stop propagation, so
/// the card never sees the click. It DOES see the `pointerdown` and the
/// `pointerup` either side of it, because those are not the event a button stops,
/// and a tap decided from that pair would open the book the reader was asking to
/// remove. So the press is handed to the control before anything is decided, which
/// is one rule here rather than a `stop_propagation` on every pointer event of
/// every control any card ever grows.
const OWNED_BY_A_CONTROL: &str = "button, a, input, select, textarea, [contenteditable]";

/// How far the pointer may travel before the press commits to a drag.
///
/// Smaller than the long-press primitive's slop (`long_press::SELECT_SLOP_PX`,
/// which is the distance a HOLD survives) on purpose: a drag has to be decided
/// before the hold it replaces would have been cancelled, or the two overlap in
/// a band where the reader gets neither. Six pixels is inside a shaky finger's
/// drift and outside a deliberate move.
pub const DRAG_THRESHOLD_PX: f64 = 6.0;

/// What the gesture decided. Locked from the moment it is anything but
/// [`Mode::Undecided`] until the pointer is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Nothing yet: the timer has not fired and the pointer has not travelled.
    Undecided,
    /// Released without travelling — the press was an open.
    Tap,
    /// The hold completed — selection owns the pointer now.
    Hold,
    /// The pointer travelled and a drag was allowed — the drag owns it.
    Drag,
    /// The pointer travelled and nothing was draggable, so the press ended
    /// without deciding anything.
    Abandoned,
}

/// What the gesture needs from the caller. Every field is a `Copy` handle, so
/// one options value can feed all four handlers without being taken apart.
pub struct DraggableItemOptions {
    /// How long a press must hold before it becomes a selection. Callers pass
    /// `long_press::SELECT_PRESS_MS`: a book on a shelf and a stroke on a page
    /// are one gesture to a reader, and two numbers with the same meaning in
    /// two directories eventually stop matching.
    pub press_ms: i32,
    /// How far the pointer travels before the press commits to a drag.
    pub drag_threshold_px: f64,
    /// Whether a drag is allowed at all. Off while the shelf is choosing: the
    /// pointer is selecting, not filing.
    pub draggable: Signal<bool>,
    /// Whether a hold may start a selection. Off once one is running, where a
    /// tap already toggles and a second gesture per card would be a second way
    /// to do the thing the tap now does.
    pub selectable: Signal<bool>,
    /// The press ended as a tap.
    pub on_tap: Callback<()>,
    /// The hold completed: enter the selection with this card already in it.
    pub on_long_press: Callback<()>,
    /// The pointer committed to a drag.
    pub on_drag_start: Callback<()>,
    /// The pointer is dragging, with its client coordinates. The second type
    /// argument is spelled out because `Callback`'s answer defaults to its
    /// question, and a stream of coordinates is not an answer anybody wants back.
    pub on_drag_move: Callback<(f64, f64), ()>,
    /// The drag ended, however it ended.
    pub on_drag_end: Callback<()>,
}

/// The four pointer handlers to spread onto the element, the two live flags a
/// card paints itself from, and the one-shot probes for the events a completed
/// hold generates.
pub struct DraggableItemHandle {
    pub on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    /// Reactive "the press is counting" flag — the tint that arrives on the
    /// frame the finger does, before anything has been decided.
    pub pressing: RwSignal<bool>,
    /// Reactive "the pointer committed to a drag" flag.
    pub dragging: RwSignal<bool>,
    /// One-shot: `true` when the click following a completed hold must be
    /// swallowed; resets on read.
    pub swallow_click: Rc<dyn Fn() -> bool>,
    /// One-shot: `true` when the synthetic contextmenu after a completed hold
    /// must be swallowed; resets on read.
    pub swallow_context: Rc<dyn Fn() -> bool>,
}

/// A pending hold timer: the JS timeout handle plus the wasm-shim closure it
/// keeps alive. Parked in a `StoredValue` so a re-run or a cleanup cannot free
/// the closure while the timeout is still queued.
type PendingTimer = Option<(i32, Closure<dyn FnMut()>)>;

/// Stop an in-flight hold. Clears the pending timer — harmless when it has
/// already fired — and drops the parked closure.
fn cancel_hold(timer: StoredValue<PendingTimer, LocalStorage>) {
    timer.with_value(|t| {
        if let Some((handle, _)) = t
            && let Some(win) = web_sys::window()
        {
            win.clear_timeout_with_handle(*handle);
        }
    });
    timer.set_value(None);
}

/// Whether the pointer has left the threshold around its origin. Both sides
/// squared, so a drag decision costs no square root on the event that arrives
/// most often; the boundary itself is still inside, which keeps a threshold of
/// zero from firing on a perfectly still pointer.
fn travelled(x: f64, y: f64, origin: (f64, f64), threshold_px: f64) -> bool {
    let dx = x - origin.0;
    let dy = y - origin.1;
    dx * dx + dy * dy > threshold_px * threshold_px
}

/// Build the gesture handlers, owned by the current reactive owner.
pub fn use_draggable_item(options: DraggableItemOptions) -> DraggableItemHandle {
    let DraggableItemOptions {
        press_ms,
        drag_threshold_px,
        draggable,
        selectable,
        on_tap,
        on_long_press,
        on_drag_start,
        on_drag_move,
        on_drag_end,
    } = options;

    let mode = StoredValue::new_local(Mode::Undecided);
    let origin = StoredValue::new_local(None::<(f64, f64)>);
    let timer: StoredValue<PendingTimer, LocalStorage> = StoredValue::new_local(None);
    let suppress_click = StoredValue::new_local(false);
    let suppress_context = StoredValue::new_local(false);
    let pressing = RwSignal::new(false);
    let dragging = RwSignal::new(false);

    let cancel: Rc<dyn Fn()> = Rc::new(move || cancel_hold(timer));
    let reset: Rc<dyn Fn()> = Rc::new({
        let cancel = Rc::clone(&cancel);
        move || {
            cancel();
            mode.set_value(Mode::Undecided);
            origin.set_value(None);
            pressing.set(false);
            dragging.set(false);
        }
    });

    let on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |ev| {
            // Only the primary button starts a gesture. A secondary one ends
            // whatever was in flight rather than joining it: the contextmenu it
            // brings is the card's to answer, and a pointerup that followed it
            // into `on_tap` would open the book the reader was asking about.
            if ev.button() != 0 {
                reset();
                return;
            }
            reset();
            let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                return;
            };
            if el.closest(OWNED_BY_A_CONTROL).ok().flatten().is_some() {
                return;
            }
            suppress_click.set_value(false);
            suppress_context.set_value(false);
            origin.set_value(Some((ev.client_x() as f64, ev.client_y() as f64)));
            pressing.set(true);

            // Capture, so the gesture survives the pointer drifting off a narrow
            // cover, and so the browser's own dragstart arrives here as a
            // `pointercancel` instead of letting the hold complete behind it.
            let _ = el.set_pointer_capture(ev.pointer_id());

            if !selectable.get_untracked() {
                return;
            }
            let Some(win) = web_sys::window() else {
                return;
            };
            let cb = Closure::<dyn FnMut()>::new(move || {
                // Decided once: a hold that fired while the pointer was already
                // dragging would be a second answer to the same press.
                if mode.get_value() != Mode::Undecided {
                    return;
                }
                mode.set_value(Mode::Hold);
                pressing.set(false);
                suppress_click.set_value(true);
                suppress_context.set_value(true);
                on_long_press.run(());
            });
            let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            if let Ok(handle) =
                win.set_timeout_with_callback_and_timeout_and_arguments_0(&f, press_ms)
            {
                timer.set_value(Some((handle, cb)));
            }
        }
    });

    let on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let cancel = Rc::clone(&cancel);
        move |ev| {
            let Some(at) = origin.get_value() else {
                return;
            };
            let point = (ev.client_x() as f64, ev.client_y() as f64);
            match mode.get_value() {
                Mode::Undecided => {
                    if !travelled(point.0, point.1, at, drag_threshold_px) {
                        return;
                    }
                    cancel();
                    pressing.set(false);
                    if draggable.get_untracked() {
                        mode.set_value(Mode::Drag);
                        dragging.set(true);
                        on_drag_start.run(());
                        on_drag_move.run(point);
                    } else {
                        mode.set_value(Mode::Abandoned);
                    }
                }
                Mode::Drag => on_drag_move.run(point),
                // A hold has already answered this press, and an abandoned one
                // has nothing left to answer with.
                Mode::Tap | Mode::Hold | Mode::Abandoned => {}
            }
        }
    });

    let on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |_ev| {
            // Not our press: no pointerdown of ours is in flight, so there is
            // nothing to decide and no tap to fire.
            if origin.get_value().is_none() {
                return;
            }
            match mode.get_value() {
                Mode::Undecided => {
                    mode.set_value(Mode::Tap);
                    on_tap.run(());
                }
                Mode::Drag => on_drag_end.run(()),
                Mode::Tap | Mode::Hold | Mode::Abandoned => {}
            }
            reset();
        }
    });

    let on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |_ev| {
            if mode.get_value() == Mode::Drag {
                on_drag_end.run(());
            }
            reset();
        }
    });

    let swallow_click: Rc<dyn Fn() -> bool> = Rc::new(move || {
        if suppress_click.get_value() {
            suppress_click.set_value(false);
            true
        } else {
            false
        }
    });

    let swallow_context: Rc<dyn Fn() -> bool> = Rc::new(move || {
        if suppress_context.get_value() {
            suppress_context.set_value(false);
            true
        } else {
            false
        }
    });

    on_cleanup(move || cancel_hold(timer));

    DraggableItemHandle {
        on_pointerdown,
        on_pointermove,
        on_pointerup,
        on_pointercancel,
        pressing,
        dragging,
        swallow_click,
        swallow_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::primitives::interactions::long_press::SELECT_SLOP_PX;

    #[test]
    fn the_threshold_is_a_radius_around_the_origin() {
        let at = (100.0, 100.0);
        assert!(!travelled(100.0, 100.0, at, 6.0));
        assert!(!travelled(106.0, 100.0, at, 6.0), "on the boundary is still a press");
        assert!(travelled(106.1, 100.0, at, 6.0));
        // Diagonal drift counts its Euclidean length, not per-axis: 5px each way
        // is 7.07px of travel and a drag, which a per-axis test would miss.
        assert!(travelled(105.0, 105.0, at, 6.0));
        assert!(!travelled(104.0, 104.0, at, 6.0));
        // And in every direction, with the same boundary on the far side: six
        // pixels left of the origin is still a press, six and a hair is a drag.
        assert!(!travelled(94.0, 100.0, at, 6.0));
        assert!(travelled(93.9, 100.0, at, 6.0));
        assert!(travelled(100.0, 93.0, at, 6.0));
    }

    #[test]
    fn a_zero_threshold_commits_on_any_drift_at_all() {
        let at = (0.0, 0.0);
        assert!(!travelled(0.0, 0.0, at, 0.0));
        assert!(travelled(0.5, 0.0, at, 0.0));
    }

    #[test]
    fn a_drag_threshold_smaller_than_the_hold_slop_leaves_no_band_between_them() {
        // The two numbers are in different modules and one rule holds them
        // together: a drag must be decided before the hold it replaces would
        // have been cancelled, so the reader is never in a band where the
        // pointer has travelled far enough to lose the selection and not far
        // enough to have a drag.
        assert!(DRAG_THRESHOLD_PX < SELECT_SLOP_PX);
    }
}
