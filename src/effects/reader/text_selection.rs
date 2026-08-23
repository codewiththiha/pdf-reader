//! Text-selection detail tracking for the AI explain feature.
//!
//! The engine's selectionchange listener debounces the native selection,
//! measures its bounding rect and grabs the surrounding sentence, then
//! dispatches a `pdfreader:selection-detail` CustomEvent with
//! `{ text, context, rect }` (viewport CSS px) — or `null` to clear.
//! Collapses caused by pressing inside the AI UI itself are suppressed
//! engine-side, so the "Info" button survives its own click.
//!
//! This effect is the single place that turns that event into writes on
//! `state.reader.ai_selection`: `detail` positions the floating menu, and a
//! genuine clear also closes an open popover.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use crate::state::reader::SelectionDetail;
use crate::state::AppState;

/// The JS protocol of the event detail: `null` (clear) or a full
/// `SelectionDetail`. The engine already debounces and dedupes, so every
/// event that arrives here is a genuine change (a `.set()` always notifies,
/// even on unchanged values).
fn parse_selection_detail(detail: &JsValue) -> Option<SelectionDetail> {
    if detail.is_null() || detail.is_undefined() {
        return None;
    }
    serde_wasm_bindgen::from_value::<SelectionDetail>(detail.clone()).ok()
}

pub fn text_selection(state: AppState) {
    let _handle = window_event_listener(
        leptos::ev::Custom::new("pdfreader:selection-detail"),
        move |ev: web_sys::CustomEvent| {
            let detail = ev.detail();
            match parse_selection_detail(&detail) {
                Some(selection) => {
                    state.reader.ai_selection.detail.set(Some(selection));
                    // A new selection supersedes any open explanation.
                    state.reader.ai_selection.popover_open.set(false);
                }
                None => {
                    state.reader.ai_selection.detail.set(None);
                    state.reader.ai_selection.popover_open.set(false);
                }
            }
        },
    );
    on_cleanup(move || _handle.remove());
}
