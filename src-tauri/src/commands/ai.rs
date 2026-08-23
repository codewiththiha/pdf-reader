use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use crate::ai::{AiChunk, create_provider};

#[tauri::command]
pub async fn explain_word(app: AppHandle, word: String, context: String) -> Result<(), String> {
    // 1. Get the provider (Apple or Mock depending on build target)
    let provider = create_provider();

    // 2. Get the stream
    let mut stream = provider.explain_word(word, context);

    // 3. Consume the stream and emit events to the frontend
    while let Some(chunk) = stream.next().await {
        // Emit the chunk to the frontend via Tauri events
        app.emit("ai-stream-chunk", &chunk).map_err(|e| e.to_string())?;

        // If it's done or an error, we can stop the loop
        if matches!(chunk, AiChunk::Done | AiChunk::Error(_)) {
            break;
        }
    }

    Ok(())
}
