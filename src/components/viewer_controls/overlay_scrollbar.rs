//! Thin floating scrollbar that takes no layout space.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::components::primitives::hooks::dom::by_id;

#[component]
pub fn OverlayScrollbar(
    scroller_id: &'static str,
    #[prop(default = false)] horizontal: bool,
) -> impl IntoView {
    let progress = RwSignal::new(0.0f64);
    let frac = RwSignal::new(1.0f64);
    let shown = RwSignal::new(false);
    let hide_handle: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    let drag = RwSignal::new(None::<(f64, i32)>);

    let read_metrics = move || -> Option<(i32, i32, i32)> {
        let el = by_id(scroller_id)?;
        Some(if horizontal {
            (el.scroll_width(), el.client_width(), el.scroll_left())
        } else {
            (el.scroll_height(), el.client_height(), el.scroll_top())
        })
    };

    let sync = move || {
        if let Some((total, client, pos)) = read_metrics() {
            let total = total as f64;
            let client = client as f64;
            frac.set((client / total.max(1.0)).min(1.0));
            progress.set((pos as f64 / (total - client).max(1.0)).clamp(0.0, 1.0));
        }
    };

    let poke = {
        let hide_handle = hide_handle.clone();
        move || {
            shown.set(true);
            if let Some(prev) = hide_handle.get() {
                if let Some(w) = web_sys::window() {
                    w.clear_timeout_with_handle(prev);
                }
            }
            let shown_hide = shown;
            let slot = hide_handle.clone();
            let cb = Closure::once_into_js(move || {
                shown_hide.set(false);
            });
            if let Some(w) = web_sys::window() {
                if let Ok(h) = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    1000,
                ) {
                    slot.set(Some(h));
                }
            }
        }
    };

    Effect::new(move |_| {
        let Some(el) = by_id(scroller_id) else {
            return;
        };
        let sync_c = sync;
        let poke_c = poke;
        let cb = Closure::wrap(Box::new(move |_: web_sys::Event| {
            sync_c();
            poke_c();
        }) as Box<dyn FnMut(web_sys::Event)>);
        let _ = el.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
        // Leak intentionally for the component lifetime — cleaned on page unmount
        // when the owner drops. Store as StoredValue so Drop runs with the owner.
        let retained = StoredValue::new_local(cb);
        on_cleanup(move || {
            if let Some(el) = by_id(scroller_id) {
                retained.with_value(|cb| {
                    let _ = el
                        .remove_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
                });
            }
        });
        sync();
    });

    let axis_pos = move |ev: &leptos::ev::PointerEvent| {
        if horizontal {
            ev.client_x() as f64
        } else {
            ev.client_y() as f64
        }
    };

    let track_base = if horizontal {
        "pointer-events-auto absolute inset-x-2 bottom-0.5 h-1.5"
    } else {
        "pointer-events-auto absolute inset-y-2 right-0.5 w-1.5"
    };

    view! {
        <div
            class=move || {
                if !shown.get() || frac.get() >= 1.0 {
                    format!("{track_base} transition-opacity duration-150 opacity-0")
                } else {
                    format!("{track_base} transition-opacity duration-150 opacity-100")
                }
            }
            on:pointerdown=move |ev| {
                if let Some((_, _, pos)) = read_metrics() {
                    drag.set(Some((axis_pos(&ev), pos)));
                    if let Some(t) = ev.current_target() {
                        if let Ok(el) = t.dyn_into::<web_sys::Element>() {
                            let _ = el.set_pointer_capture(ev.pointer_id());
                        }
                    }
                    shown.set(true);
                }
            }
            on:pointermove=move |ev| {
                if let (Some((p0, s0)), Some((total, client, _))) = (drag.get(), read_metrics()) {
                    let delta =
                        (axis_pos(&ev) - p0) / (client as f64).max(1.0) * total as f64;
                    let next = (s0 as f64 + delta) as i32;
                    if let Some(el) = by_id(scroller_id) {
                        if horizontal {
                            el.set_scroll_left(next);
                        } else {
                            el.set_scroll_top(next);
                        }
                    }
                }
            }
            on:pointerup=move |_| drag.set(None)
            on:pointercancel=move |_| drag.set(None)
        >
            <div
                class="absolute rounded-full bg-white/30 hover:bg-white/50"
                style=move || {
                    let len = (frac.get() * 100.0).max(8.0);
                    let off = progress.get() * (100.0 - len);
                    if horizontal {
                        format!("left:{off}%;width:{len}%;top:0;bottom:0")
                    } else {
                        format!("top:{off}%;height:{len}%;left:0;right:0")
                    }
                }
            ></div>
        </div>
    }
}
