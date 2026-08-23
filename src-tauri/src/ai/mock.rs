use async_stream::stream;
use futures::Stream;
use std::pin::Pin;
use std::time::Duration;

use super::{
    schema::WordInfo,
    traits::{AiChunk, AiProvider},
};

pub struct MockAiProvider;

impl MockAiProvider {
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }
}

impl AiProvider for MockAiProvider {
    fn explain_word(
        &self,
        word: String,
        _context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>> {
        Box::pin(stream! {
            // Simulate AI "thinking" delay so you can test the rainbow glow animation
            tokio::time::sleep(Duration::from_millis(800)).await;

            yield AiChunk::Snapshot(WordInfo {
                pos: "noun".to_string(),
                meaning: format!("(Mock) A simulated, simplified meaning for the word '{}'.", word),
                synonyms: vec!["mock-synonym-1".to_string(), "mock-synonym-2".to_string()],
                usages: vec![
                    format!("This is the first mock example using the word {}.", word),
                    format!("Here is another context where {} is used.", word),
                ],
            });

            // Simulate streaming delay
            tokio::time::sleep(Duration::from_millis(400)).await;
            yield AiChunk::Done;
        })
    }
}
