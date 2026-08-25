//! The toolbar document-name label. Its width is the actual space between
//! the leading controls and trailing toolbar cluster; it deliberately
//! measures DOM rects rather than reproducing title-bar padding or
//! sidebar-state assumptions.

use leptos::prelude::*;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use pdf_core::filename::display_name;
use crate::state::AppState;

/// Gap after the leading controls (`gap-1`).
const GAP_LEFT: f64 = 4.0;
/// Breathing room before the trailing control cluster.
const GAP_RIGHT: f64 = 12.0;
const TITLE_MIN_LABEL_W: f64 = crate::components::app_shell::constants::MIN_DOC_TITLE_WIDTH;

/// Measure the title's real slot in toolbar-row coordinates. This remains
/// correct through the sidebar close slide because it uses the live rects,
/// not the raw sidebar mode or title-bar padding model.
fn measure_available() -> Option<f64> {
    let doc = web_sys::window()?.document()?;
    let row = doc.get_element_by_id("toolbar-row")?;
    let row_rect = row.get_bounding_client_rect();
    if row_rect.width() <= 0.0 {
        return None;
    }
    let pre = doc.get_element_by_id("toolbar-left-pre")?;
    let pre_rect = pre.get_bounding_client_rect();
    let right = doc.get_element_by_id("toolbar-right")?;
    let right_rect = right.get_bounding_client_rect();

    // The pin button follows #toolbar-right inside its ml-auto parent. Using
    // the trailing group's left edge therefore reserves it automatically.
    let start = pre_rect.right() - row_rect.left() + GAP_LEFT;
    let end = right_rect.left() - row_rect.left() - GAP_RIGHT;
    Some((end - start).max(0.0))
}

#[component]
pub fn DocumentTitle(state: AppState) -> impl IntoView {
    let avail = RwSignal::new(None::<f64>);
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    let remeasure = move || {
        request_animation_frame(move || {
            if let Some(w) = measure_available() {
                let prev = avail.get_untracked();
                if prev.is_none_or(|p: f64| (p - w).abs() > 0.5) {
                    avail.set(Some(w));
                }
            }
        });
    };
    let remeasure_for_ro = remeasure.clone();

    // The route/page mount order may put this effect ahead of its anchor ids.
    // Do not mark installation complete until at least one live anchor was
    // observed; subsequent reactive runs then self-heal that initial miss.
    let try_install = move || {
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |_: Vec<ResizeObserverEntry>| {
                remeasure_for_ro();
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) else {
            return;
        };
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };

        let mut observed = false;
        for id in ["toolbar-row", "toolbar-left-pre", "toolbar-right"] {
            if let Some(el) = doc.get_element_by_id(id) {
                observer.observe(&el);
                observed = true;
            }
        }
        // The title row changes inset at the end of the close hold, whereas
        // this aside changes width during every frame of the 300ms slide.
        if let Some(aside) = doc.query_selector("aside.sidebar-aside").ok().flatten() {
            observer.observe(&aside);
            observed = true;
        }
        if observed {
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        } else {
            observer.disconnect();
        }
    };

    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    Effect::new(move |_| {
        try_install();
        _ = state.reader.document.status.get();
        _ = state.reader.document.num_pages.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        _ = state.ui.sidebar.get();
        remeasure();
    });

    let name = move || {
        display_name(
            state.reader.document.title.get().as_deref(),
            state.reader.document.path.get().as_deref(),
        )
        .unwrap_or_else(|| "No document".to_string())
    };
    let full = move || name();
    let hidden = move || avail.get().is_some_and(|w| w < TITLE_MIN_LABEL_W);

    view! {
        <span
            id="toolbar-doc-title"
            data-tauri-drag-region="true"
            class="min-w-0 shrink truncate text-sm text-ink"
            class=("hidden", hidden)
            title=full
            style:max-width=move || match avail.get() {
                Some(w) if w >= TITLE_MIN_LABEL_W => format!("{}px", w.floor()),
                Some(_) => "0px".to_string(),
                None => "none".to_string(),
            }
        >
            {name}
        </span>
    }
}
