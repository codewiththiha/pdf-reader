//! The frontend half of the AI word-explanation feature.
//!
//! Owns the wire protocol with the Tauri backend: [`invoke_explain_word`]
//! starts a run (fire-and-forget — results never come back through the
//! invoke) and [`listen_ai_chunks`] decodes the streamed `ai-stream-chunk`
//! events back into typed chunks. Components never see a `JsValue`.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use wasm_bindgen::JsValue;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::Event;

use crate::components::ai::types::WordInfo;

/// The wire format of one `ai-stream-chunk` payload. Mirrors `AiChunk` in
/// `src-tauri/src/ai/traits.rs` (same `type`/`data` tagging) — keep in sync.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunkEvent {
    Snapshot(WordInfo),
    Done,
    Error(String),
}

/// Starts an `explain_word` run on the backend. The streamed results arrive
/// as `ai-stream-chunk` events picked up by [`listen_ai_chunks`].
pub fn invoke_explain_word(word: String, context: String) {
    spawn_local(async move {
        if let Err(e) = pdf_engine::api::explain_word(&word, &context).await {
            web_sys::console::warn_1(&format!("[ai] explain_word invoke failed: {e}").into());
        }
    });
}

/// Subscribe to the backend's `ai-stream-chunk` events.
///
/// The JS Closure is parked in the returned StoredValue: keep that value
/// alive for as long as chunks should be received (it is disposed with the
/// owning component). Same registration pattern as the drag-drop listeners
/// in `effects/app/drag_drop.rs` — the Tauri unlisten handle is
/// deliberately discarded.
pub fn listen_ai_chunks(
    on_chunk: impl Fn(AiChunkEvent) + 'static,
) -> StoredValue<Option<Closure<dyn FnMut(Event)>>, LocalStorage> {
    let handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);

    let cb: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |ev: Event| {
            // Tauri event object; the AiChunk payload is under `.payload`.
            let value: &JsValue = ev.as_ref();
            let payload = js_sys::Reflect::get(value, &"payload".into())
                .unwrap_or(JsValue::UNDEFINED);
            match serde_wasm_bindgen::from_value::<AiChunkEvent>(payload) {
                Ok(chunk) => on_chunk(chunk),
                Err(e) => {
                    web_sys::console::warn_1(&format!("[ai] bad chunk payload: {e}").into());
                }
            }
        }) as Box<dyn FnMut(Event)>,
    );

    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        _ = pdf_engine::listen("ai-stream-chunk", f).await;
    });
    handle.set_value(Some(cb));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact JSON shapes the backend's `app.emit("ai-stream-chunk", _
    // &chunk)` produces from `AiChunk`'s serde tagging. If these parse, the
    // enum mirror above is wire-compatible.
    #[test]
    fn chunk_wire_shapes_parse() {
        let snapshot: AiChunkEvent = serde_json::from_str(
            r#"{"type":"Snapshot","data":{"pos":"noun","meaning":"lasting briefly","synonyms":["fleeting"],"usages":["ephemeral beauty"]}}"#,
        )
        .unwrap();
        match snapshot {
            AiChunkEvent::Snapshot(info) => {
                assert_eq!(info.pos, "noun");
                assert_eq!(info.synonyms, vec!["fleeting"]);
            }
            other => panic!("expected Snapshot, got {other:?}"),
        }

        let done: AiChunkEvent = serde_json::from_str(r#"{"type":"Done"}"#).unwrap();
        assert!(matches!(done, AiChunkEvent::Done));

        let error: AiChunkEvent =
            serde_json::from_str(r#"{"type":"Error","data":"model unavailable"}"#).unwrap();
        match error {
            AiChunkEvent::Error(msg) => assert_eq!(msg, "model unavailable"),
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
