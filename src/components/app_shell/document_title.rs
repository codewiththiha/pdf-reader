//! The toolbar document-name label. Its width is the actual space between
//! the leading controls and trailing toolbar cluster; it deliberately
//! measures DOM rects rather than reproducing title-bar padding or
//! sidebar-state assumptions.

use leptos::prelude::*;

use crate::components::primitives::hooks::use_resize_observer::observe_elements;

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
    let pre = doc.get_element_by_id("toolbar-leading")?;
    let pre_rect = pre.get_bounding_client_rect();
    let right = doc.get_element_by_id("toolbar-trailing")?;
    let right_rect = right.get_bounding_client_rect();

    // The pin button follows #toolbar-trailing inside its ml-auto parent. Using
    // the trailing group's left edge therefore reserves it automatically.
    let start = pre_rect.right() - row_rect.left() + GAP_LEFT;
    let end = right_rect.left() - row_rect.left() - GAP_RIGHT;
    Some((end - start).max(0.0))
}

#[component]
pub fn DocumentTitle(state: AppState) -> impl IntoView {
    let avail = RwSignal::new(None::<f64>);

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

    // The route/page mount order may put this effect ahead of its anchor
    // ids; the primitive installs once with the first non-empty set and a
    // later reactive run self-heals an initial miss.
    Effect::new(move |_| {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let mut els = Vec::new();
        for id in ["toolbar-row", "toolbar-leading", "toolbar-trailing"] {
            if let Some(el) = doc.get_element_by_id(id) {
                els.push(el);
            }
        }
        // The title row changes inset at the end of the close hold, whereas
        // this aside changes width during every frame of the 300ms slide.
        if let Some(aside) = doc.query_selector("aside.sidebar-aside").ok().flatten() {
            els.push(aside);
        }
        observe_elements(els, move |_| remeasure());
    });

    Effect::new(move |_| {
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
