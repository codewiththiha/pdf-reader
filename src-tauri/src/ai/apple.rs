use async_stream::stream;
use fm_bridge::{Bridge, Request, StreamEvent};
use futures::{Stream, StreamExt};
use std::pin::Pin;

use super::{
    prompts::{WORD_INFO_SYSTEM_PROMPT, WORD_INFO_USER_PROMPT},
    schema::{word_info_schema, WordInfo},
    traits::{AiChunk, AiProvider},
};

pub struct AppleAiProvider {
    bridge: Bridge,
}

impl AppleAiProvider {
    pub fn new() -> Result<Self, String> {
        // Reads FM_BRIDGE_BIN from the .env file
        let bridge = Bridge::from_env().map_err(|e| e.to_string())?;

        // Optional: Increase concurrency if you expect multiple lookups
        let bridge = bridge.max_concurrency(2);

        Ok(Self { bridge })
    }
}

impl AiProvider for AppleAiProvider {
    fn explain_word(
        &self,
        word: String,
        context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>> {
        let bridge = self.bridge.clone();
        let schema = word_info_schema();

        let prompt = WORD_INFO_USER_PROMPT
            .replace("{word}", &word)
            .replace("{context}", &context);

        let request = Request::new()
            .system(WORD_INFO_SYSTEM_PROMPT)
            .user(&prompt)
            .schema(schema)
            .stream_structured(true); // Enable streaming of JSON snapshots

        Box::pin(stream! {
            let mut stream = Box::pin(bridge.stream(request));

            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::Snapshot(val)) | Ok(StreamEvent::Structured(val)) => {
                        // Parse the JSON value into our WordInfo struct
                        if let Ok(info) = serde_json::from_value::<WordInfo>(val) {
                            yield AiChunk::Snapshot(info);
                        }
                    }
                    Ok(StreamEvent::Done(_)) => {
                        yield AiChunk::Done;
                        break;
                    }
                    Err(e) => {
                        yield AiChunk::Error(e.to_string());
                        break;
                    }
                    _ => {} // Ignore other events
                }
            }
        })
    }
}
