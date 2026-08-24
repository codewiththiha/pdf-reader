//! Pointer physics for dragging the expanded card. Writes an anchor-relative
//! offset (owned by the [`GlossController`]) so the card keeps gliding with
//! the page on scroll: target = f(live_anchor) + offset.

use leptos::prelude::*;
use pdf_core::gloss::GlossBox;
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::placement::CARD_MARGIN;
use crate::components::ai::gloss::util::viewport_size;

pub struct CardDrag {
    /// Hand to the surface's drag handle: `(client_x, client_y, box_now)`.
    pub on_drag_start: Callback<(f64, f64, GlossBox)>,
}

pub fn use_card_drag(ctrl: GlossController, expanded: Memo<Option<GlossBox>>) -> CardDrag {
    let on_drag_start = Callback::new(move |(cx, cy, origin): (f64, f64, GlossBox)| {
        ctrl.grab.set_value(Some((cx - origin.x, cy - origin.y)));
        ctrl.dragging.set(true);
    });

    Effect::new(move |_| {
        if !ctrl.dragging.get() {
            return;
        }

        let mv = window_event_listener_untyped("pointermove", move |ev: web_sys::Event| {
            let me = ev.unchecked_ref::<web_sys::MouseEvent>();
            let Some((dx, dy)) = ctrl.grab.get_value() else {
                return;
            };
            let Some(e) = expanded.get_untracked() else {
                return;
            };
            let (vw, vh) = viewport_size();
            let x = (me.client_x() as f64 - dx).clamp(CARD_MARGIN, vw - e.w - CARD_MARGIN);
            let y = (me.client_y() as f64 - dy).clamp(CARD_MARGIN, vh - e.h - CARD_MARGIN);
            ctrl.drag_offset.set(Some((x - e.x, y - e.y)));
        });

        let end = move || {
            ctrl.grab.set_value(None);
            ctrl.dragging.set(false);
        };
        let up = window_event_listener_untyped("pointerup", move |_| end());
        let cancel = window_event_listener_untyped("pointercancel", move |_| end());

        on_cleanup(move || {
            mv.remove();
            up.remove();
            cancel.remove();
        });
    });

    CardDrag { on_drag_start }
}
