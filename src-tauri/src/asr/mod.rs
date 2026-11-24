//! Speech recognition with the bundled whisper.cpp and turning words into readable cues.

pub mod segment;
pub mod whisper;

/// One recognized word (or word-like chunk) with its time on the original timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Text including its leading space as produced by Whisper (" word").
    pub text: String,
}
