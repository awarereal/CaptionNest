//! Speech recognition with the bundled whisper.cpp and turning words into readable cues.

pub mod align;
pub mod prompt;
pub mod resync;
pub mod sarvam;
pub mod script;
pub mod segment;
pub mod vad;
pub mod whisper;

/// One recognized word (or word-like chunk) with its time on the original timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Text including its leading space as produced by Whisper (" word").
    pub text: String,
    /// Whisper's confidence in this word (lowest letter probability, 0..1).
    pub confidence: f32,
}
