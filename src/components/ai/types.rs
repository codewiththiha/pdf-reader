use serde::Deserialize;

/// Geometry of the word card. Orthogonal to [`AiPhase`] (the AI data status):
/// `AiPhase` says what the *data* is doing, `GlossPhase` says what the
/// *card's box* is doing. The two are decoupled so the card body can stream
/// text while the box independently blooms / expands / folds back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlossPhase {
    /// Small padded box hugging the selected word, accent bloom pulsing — the
    /// beat before the card blooms out.
    #[default]
    Processing,
    /// The full card, sprung open from the word.
    Expanded,
    /// Folded back onto the word (scroll-to-close) — a chip the reader can
    /// hover/tap to re-open in place.
    Compact,
}

/// The data status of the AI explanation: what the *content* of the card is
/// doing, independent of the card's geometry phase ([`GlossPhase`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// No AI activity. The card is closed.
    #[default]
    Idle,
    /// The AI request has been sent; waiting for the first token. The card is
    /// showing the accent bloom beat.
    Processing,
    /// Tokens are arriving. The card is open and text is streaming in with
    /// blur-fade.
    Streaming,
    /// All data received. Final state.
    Done,
    /// Something went wrong.
    Error,
}

/// The structured word information returned by the AI.
/// Mirrors the backend `WordInfo` struct.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WordInfo {
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}
