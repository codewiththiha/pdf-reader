use futures::Stream;
use std::pin::Pin;

use super::schema::WordInfo;

/// The chunks of data we will stream to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    /// A partial or final snapshot of the WordInfo
    Snapshot(WordInfo),
    /// The stream is complete
    Done,
    /// An error occurred.
    /// Only produced by providers that can actually fail mid-stream (the
    /// Apple provider, or the bridge failing to start); the mock never
    /// errors, which fallback builds would call dead code.
    #[allow(dead_code)]
    Error(String),
}

/// The trait that all AI providers must implement.
/// We use a pinned boxed stream to allow async streaming without lifetime hell.
pub trait AiProvider: Send + Sync {
    fn explain_word(
        &self,
        word: String,
        context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>>;
}
