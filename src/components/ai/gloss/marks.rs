//! The persistent gloss highlight layer.
//!
//! One of these is rendered by every `.pdf-page` host, alongside the canvas
//! and the text layer. Because Leptos owns it, it is re-created whenever the
//! page mounts — which is what makes a mark survive the virtualizer's
//! unmounting, a zoom's textLayer rebuild, a view-mode flip and (through
//! localStorage) the session itself. Marks are page-space rects, so the only
//! thing that changes on zoom is the multiplication by `scale`.
//!
//! Clicking a mark re-opens the card for that word. The click travels as a
//! `pdfreader:gloss-open` CustomEvent rather than a callback prop: the popover
//! lives at the reader-page level, far above the page hosts, and threading a
//! callback through `PageList`/`SinglePageView`/`PageCanvas` would couple
//! three view layers to the AI feature for one message (the same reasoning as
//! `pdfreader:navigate` in `effects::reader::link_navigation`).

use leptos::prelude::*;
use pdf_core::gloss::GlossMark;

/// Name of the "a persisted mark was clicked" event.
pub const GLOSS_OPEN_EVENT: &str = "pdfreader:gloss-open";

#[component]
pub fn GlossMarkLayer(
    /// 1-based page this host renders; the layer paints only its own marks.
    page: u32,
    /// Every mark of the open document.
    marks: Signal<Vec<GlossMark>>,
    /// The display scale, so the rects follow a zoom for free.
    scale: ReadSignal<f64>,
) -> impl IntoView {
    view! {
        <div class="glossLayer" aria-hidden="false">
            <For
                each=move || {
                    marks.get().into_iter().filter(|m| m.page == page).collect::<Vec<_>>()
                }
                key=|m: &GlossMark| m.id.clone()
                children=move |m: GlossMark| {
                    let rect = m.rect;
                    let style = move || {
                        let s = scale.get();
                        format!(
                            "left:{}px;top:{}px;width:{}px;height:{}px",
                            rect.x * s,
                            rect.y * s,
                            rect.w * s,
                            rect.h * s,
                        )
                    };
                    let mark = m.clone();
                    view! {
                        <button
                            type="button"
                            class="gloss-mark"
                            title=mark.word.clone()
                            aria-label=format!("Explain {}", mark.word)
                            style=style
                            // Keep the document selection (and the page's own
                            // press handling) out of a mark click.
                            on:mousedown=move |ev| ev.prevent_default()
                            on:click=move |ev| {
                                ev.stop_propagation();
                                dispatch_open(&mark);
                            }
                        />
                    }
                }
            />
        </div>
    }
}

/// Tell the popover to re-open on `mark`.
fn dispatch_open(mark: &GlossMark) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(detail) = serde_wasm_bindgen::to_value(mark) else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(GLOSS_OPEN_EVENT, &init) {
        let _ = win.dispatch_event(&ev);
    }
}
