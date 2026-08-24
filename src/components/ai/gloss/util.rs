//! Small DOM/viewport helpers for the gloss card. Kept here (not inlined in
//! the popover) so the state machine reads as behaviour, not plumbing.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use leptos::prelude::*;

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
