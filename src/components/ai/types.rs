use serde::Deserialize;

/// The visual phase of the AI popover. Drives which CSS classes are
/// applied to the warp window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// No AI activity. The popover is closed.
    #[default]
    Idle,
    /// The AI request has been sent; waiting for the first token.
    /// Rainbow glow is active around the selection bounding box.
    Processing,
    /// Tokens are arriving. The window has morphed into the popover
    /// and text is streaming in with blur-fade.
    Streaming,
    /// All data received. Final state.
    Done,
    /// Something went wrong. Only constructed once the backend stream is
    /// wired up; the simulated flow never fails, so for now it's dead code.
    #[allow(dead_code)]
    Error,
}

impl AiPhase {
    /// The CSS class to apply to the warp window.
    pub fn css_class(&self) -> &'static str {
        match self {
            AiPhase::Idle => "",
            AiPhase::Processing => "ai-processing",
            AiPhase::Streaming => "ai-streaming",
            AiPhase::Done => "ai-done",
            AiPhase::Error => "ai-error",
        }
    }
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
