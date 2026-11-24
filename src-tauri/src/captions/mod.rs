//! Caption data, SRT timestamps, SRT serialization and export validation.

pub mod srt;
pub mod timefmt;
pub mod validate;

use serde::{Deserialize, Serialize};

/// One caption cue on the original video's timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

impl Cue {
    pub fn new(start_ms: i64, end_ms: i64, text: impl Into<String>) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), start_ms, end_ms, text: text.into() }
    }
}
