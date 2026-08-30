//! Measure the real content height of an invisible "twin" node so a floating
//! card can size itself to its content (no minimum: a two-line answer gets a
//! two-line card, and content growing the twin later animates the card open a
//! little further). The twin must be a pixel-exact replica of the card's
//! scroll column.
//!
//! Two triggers keep the measurement honest so the card can NEVER under-size
//! itself (which is what shows up as a scroll cursor the user has to reopen
//! to clear):
//! * a reactive one — when a tracked signal changes (a streamed snapshot
//!   patches content in, the word wraps differently), the effect defers one
//!   frame and re-reads the twin's `scroll_height`;
//! * a layout one — a `ResizeObserver` on the twin fires whenever the twin's
//!   own box actually changes height (a font swap, a section appearing, the
//!   answer streaming in), independent of whether a tracked signal changed.
//!   This is what covers the very first appearance, where the twin is read
//!   before content is present and the deferred reactive read is missed:
//!   without it the card opens at the *pre-content* height and only grows on
//!   a re-open.

use leptos::{html, prelude::*};

use super::use_resize_observer::use_resize_observer;

/// How much the twin's height must move before it is worth pushing to the
/// card. A 1px change is noise (the browser sub-pixel rounding); re-flowing
/// the card's spring for less than 2px would just throb.
const JITTER_PX: f64 = 2.0;

/// A deferred, jitter-guarded write of the twin's current height to the
/// signal. Deferred one frame so the twin has already reflected the new
/// content by the time it is read.
fn write_height(el: &web_sys::HtmlDivElement, content_height: RwSignal<f64>) {
    // Clone into the deferred closure: it must be `'static`, and the borrow
    // it wraps is scoped to this call.
    let el = el.clone();
    request_animation_frame(move || {
        let next = el.scroll_height() as f64;
        content_height.update(|h| {
            if (*h - next).abs() > JITTER_PX {
                *h = next;
            }
        });
    });
}

/// Returns the live height signal fed by the measure twin.
///
/// `read` is invoked inside the tracking effect, so callers express which
/// signals should trigger a re-measure by simply reading them there (e.g.
/// `move || { let _ = info.get(); let _ = word.get(); }`).
pub fn use_content_size(measure_ref: NodeRef<html::Div>, read: impl Fn() + 'static) -> RwSignal<f64> {
    let content_height = RwSignal::new(0.0_f64);

    // Reactive trigger: any tracked signal that drives the twin's content.
    Effect::new(move |_| {
        read();
        if let Some(el) = measure_ref.get() {
            write_height(&el, content_height);
        }
    });

    // Layout backstop: whatever changes the twin's height — a streamed
    // snapshot, a font swap, the width returning from a clamp — opens/keeps
    // the card at its real size. Deleting this is why the card used to need
    // a close-and-reopen to grow on first appearance.
    let observer_ref = measure_ref;
    use_resize_observer(observer_ref, move |_| {
        if let Some(el) = measure_ref.get() {
            write_height(&el, content_height);
        }
    });

    content_height
}
