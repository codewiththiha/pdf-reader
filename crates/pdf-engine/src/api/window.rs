//! The AI word-explanation kickoff.
//!
//! (The window commands that shared this module — traffic lights, minimize,
//! maximize, close — are chrome, not engine: they live in the `app-chrome`
//! crate's `window::api`.)

use wasm_bindgen::JsValue;

use super::{reflect_set, KEY_CONTEXT, KEY_RUN, KEY_WORD};

/// Fire-and-forget start of an `explain_word` run on the backend. Nothing
/// comes back here: the backend streams its chunks over the
/// `ai-stream-chunk` event, which the frontend AI service listens to.
/// Outside Tauri (`trunk serve`) there is nothing to invoke, so this is a
/// silent no-op; a genuine invoke failure surfaces as `Err` for the caller
/// to log.
///
/// `run` is the caller's id for this request; the backend echoes it on every
/// chunk so the listener can drop the chunks of a run it has moved on from.
pub async fn explain_word(word: &str, context: &str, run: &str) -> Result<(), String> {
    if !tauri_bridge::has_tauri() {
        return Ok(());
    }
    let args: JsValue = js_sys::Object::new().into();
    _ = reflect_set(&args, &KEY_WORD, &JsValue::from_str(word));
    _ = reflect_set(&args, &KEY_CONTEXT, &JsValue::from_str(context));
    _ = reflect_set(&args, &KEY_RUN, &JsValue::from_str(run));
    tauri_bridge::invoke("explain_word", args)
        .await
        .map(|_| ())
        .map_err(|e| e.as_string().unwrap_or_else(|| "unknown invoke error".to_string()))
}
