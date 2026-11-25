//! Offline translation of caption text with Argos Translate language packages,
//! run directly through CTranslate2 (no Python), plus route planning between languages.

pub mod argos;
pub mod routes;

/// Maps a Whisper language code to the code Argos packages use.
pub fn argos_code(whisper_code: &str) -> &str {
    match whisper_code {
        "no" | "nn" => "nb",
        "jw" => "jv",
        other => other,
    }
}
