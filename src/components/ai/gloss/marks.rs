//! The persistent gloss highlighter stroke layer.
//!
//! One of these is rendered by every `.pdf-page` host, alongside the canvas
//! and the text layer. Because Leptos owns it, it is re-created whenever the
//! page mounts — which is what makes a mark survive the virtualizer's
//! unmounting, a zoom's textLayer rebuild, a view-mode flip and (through
//! localStorage) the session itself. Marks are page-space rects, so the only
//! thing that changes on zoom is the multiplication by `scale`.
//!
//! This stroke is the reader's ONLY highlight for a glossed word: the native
//! `::selection` tint is cleared when the gloss takes over, and the morphing
//! surface unmounts once it has settled back onto this box, so nothing ever
//! stacks on top of it. It is also the whole "thinking" UI — while the model
//! is working the stroke wears `gloss-mark-processing` (drift + sweep + pulsing
//! halo) and no surface exists at all.
//!
//! Clicking a stroke re-opens the card for that word. The click travels as a
//! `pdfreader:gloss-open` CustomEvent rather than a callback prop: the popover
//! lives at the reader-page level, far above the page hosts, and threading a
//! callback through `PageList`/`SinglePageView`/`PageCanvas` would couple
//! three view layers to the AI feature for one message (the same reasoning as
//! `pdfreader:navigate` in `effects::reader::link_navigation`).

use leptos::prelude::*;
use pdf_core::gloss::GlossMark;

/// Name of the "a persisted mark was clicked" event.
pub const GLOSS_OPEN_EVENT: &str = "pdfreader:gloss-open";

/// Exact-fit stroke radius. Shared with `anchor::mark_screen_box` so the
/// morphing surface settles onto EXACTLY the box the stroke occupies — one
/// geometry. No hug-padding: the stroke is the stored union rect itself.
pub const MARK_RADIUS: f64 = 3.0;

#[component]
pub fn GlossMarkLayer(
    /// 1-based page this host renders; the layer paints only its own marks.
    page: u32,
    /// Every mark of the open document.
    marks: Signal<Vec<GlossMark>>,
    /// The display scale, so the rects follow a zoom for free.
    scale: ReadSignal<f64>,
    /// Id of the mark currently waiting on the model, if any.
    processing: Signal<Option<String>>,
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
                    // Exact-fit stroke: the stored union rect itself, no padding.
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
                    let is_processing = {
                        let id = m.id.clone();
                        move || processing.get().as_deref() == Some(id.as_str())
                    };
                    let mark = m.clone();
                    view! {
                        <button
                            type="button"
                            class="gloss-mark"
                            class=("gloss-mark-processing", is_processing)
                            title=mark.word.clone()
                            aria-label=format!("Explain {}", mark.word)
                            style=style
                            // Keep the document selection (and the page's own
                            // press handling) out of a stroke click.
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
