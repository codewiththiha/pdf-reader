//! Small DOM/viewport helpers for the gloss card. Kept here (not inlined in
//! the popover) so the state machine reads as behaviour, not plumbing.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use leptos::prelude::*;

use crate::components::document::dom_helpers::page_list;

/// The viewport size in CSS pixels. Read off `documentElement`'s bounding rect
/// rather than `window.innerWidth/Height` so it never depends on the "any"
/// return type of those getters and is unaffected by internal scrollers (this
/// app never lets the window itself scroll — `#page-list` does).
pub fn viewport_size() -> (f64, f64) {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .map(|el| {
            let r = el.get_bounding_client_rect();
            (r.width(), r.height())
        })
        .unwrap_or((0.0, 0.0))
}

/// The reader's current vertical scroll. Continuous mode reads `#page-list`'s
/// scrollTop; single-page mode has no scroller and returns 0 (so scroll-to-close
/// can't false-trigger there — a page flip collapses the card instead).
pub fn current_scroll_y() -> f64 {
    page_list().map(|el| el.scroll_top() as f64).unwrap_or(0.0)
}

/// True when the OS asks for reduced motion (a non-reactive snapshot).
pub fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

/// A reactive `prefers-reduced-motion` signal: read once, then kept in sync
/// by a `change` listener on the underlying `MediaQueryList`.
///
/// The JS objects live in `StoredValue`s for the owner's lifetime (the same
/// registration pattern as `dom_helpers`). The `Closure` (not `Clone`) is held
/// in its own slot and only cleared; the `Clone`-able `MediaQueryList` and
/// callback `Function` are retrieved *inside* the `Send + Sync` cleanup so the
/// listener is removed before the closure is freed.
pub fn reduced_motion_signal() -> RwSignal<bool> {
    let s = RwSignal::new(prefers_reduced_motion());
    let mql_store = StoredValue::new_local(None::<web_sys::MediaQueryList>);
    let cb_store = StoredValue::new_local(None::<Closure<dyn FnMut()>>);
    let f_store = StoredValue::new_local(None::<js_sys::Function>);
    Effect::new(move |_| {
        let Some(mql) = web_sys::window()
            .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
            .flatten()
        else {
            return;
        };
        let mql_for_cb = mql.clone();
        let cb: Closure<dyn FnMut()> = Closure::new(move || s.set(mql_for_cb.matches()));
        let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = mql.add_event_listener_with_callback("change", &f);
        mql_store.set_value(Some(mql));
        cb_store.set_value(Some(cb));
        f_store.set_value(Some(f));
    });
    on_cleanup(move || {
        if let (Some(mql), Some(f)) = (
            mql_store.try_get_value().flatten(),
            f_store.try_get_value().flatten(),
        ) {
            let _ = mql.remove_event_listener_with_callback("change", &f);
        }
        let _ = cb_store.try_set_value(None);
    });
    s
}

/// Add a capture-phase listener on `window`, owned by the current reactive
/// owner. Scroll events do not bubble, but they DO hit window listeners in the
/// capture phase, so one such listener catches `#page-list`, the single-page
/// container and the window alike.
///
/// Must be called from inside a reactive scope (an `Effect`); the listener is
/// torn down when that scope's owner is cleaned up (mirrors `dom_helpers`).
pub fn add_window_capture_listener(event: &str, handler: impl FnMut(web_sys::Event) + 'static) {
    let cb: Closure<dyn FnMut(web_sys::Event)> =
        Closure::wrap(Box::new(handler) as Box<dyn FnMut(web_sys::Event)>);
    let win = web_sys::window().expect("window");
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    let _ = win.add_event_listener_with_callback_and_bool(event, &f, true);

    let cb_store = StoredValue::new_local(Some(cb));
    let f_store = StoredValue::new_local(Some(f));
    let event_owned = event.to_string();
    on_cleanup(move || {
        if let Some(f) = f_store.try_get_value().flatten() {
            if let Some(win) = web_sys::window() {
                let _ = win.remove_event_listener_with_callback_and_bool(&event_owned, &f, true);
            }
        }
        let _ = cb_store.try_set_value(None);
    });
}

/// `true` if a wheel event should be absorbed by the card's own scroller rather
/// than collapse the card: the target is inside `[data-gloss-scroll]` AND that
/// scroller can still scroll in the wheel's direction (not pinned at the
/// matching edge). This is the reference's exact edge rule.
pub fn card_scroller_can_absorb(ev: &web_sys::WheelEvent) -> bool {
    let Some(target) = ev.target() else {
        return false;
    };
    let Some(el) = target.dyn_ref::<web_sys::Element>() else {
        return false;
    };
    let Some(scroller) = el.closest("[data-gloss-scroll]").ok().flatten() else {
        return false;
    };
    let st = scroller.scroll_top() as f64;
    let sh = scroller.scroll_height() as f64;
    let ch = scroller.client_height() as f64;
    let at_top = st <= 0.0;
    let at_bottom = (sh - ch - st) < 1.0;
    let dy = ev.delta_y();
    (dy > 0.0 && !at_bottom) || (dy < 0.0 && !at_top)
}

/// `true` if a touch event's target is inside the card's own scroller (so the
/// gesture should scroll the card, not collapse it).
pub fn target_inside_card_scroller(ev: &web_sys::Event) -> bool {
    ev.target()
        .and_then(|t| t.dyn_ref::<web_sys::Element>().cloned())
        .and_then(|el| el.closest("[data-gloss-scroll]").ok().flatten())
        .is_some()
}
