use std::sync::OnceLock;

use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use crate::ai::{create_provider, AiChunk, AiProvider};

/// One provider for the process's lifetime. `create_provider` reads the env
/// and builds the bridge (with its shared concurrency budget); doing that on
/// every invoke wasted both.
static PROVIDER: OnceLock<Box<dyn AiProvider>> = OnceLock::new();

fn provider() -> &'static dyn AiProvider {
    PROVIDER.get_or_init(create_provider).as_ref()
}

#[tauri::command]
pub async fn explain_word(app: AppHandle, word: String, context: String) -> Result<(), String> {
    let mut stream = provider().explain_word(word, context);

    while let Some(chunk) = stream.next().await {
        app.emit("ai-stream-chunk", &chunk).map_err(|e| e.to_string())?;
        if matches!(chunk, AiChunk::Done | AiChunk::Error(_)) {
            break;
        }
    }

    Ok(())
}
