//! The pinned model catalog, compiled into the app.
//!
//! Every entry carries the exact download URL, size and SHA-256, so downloads are
//! verified and the app never depends on a remote index at runtime.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModel {
    pub id: String,
    pub file: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
    /// Supports Whisper's speech-to-English translation task.
    pub can_translate: bool,
    /// "transcribe" (transcription only) or "both".
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgosPackage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub from_name: Option<String>,
    pub to_name: Option<String>,
    pub version: Option<String>,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub tokenizer: String,
    pub supported: bool,
    #[serde(default)]
    pub target_prefix: String,
}

#[derive(Debug, Deserialize)]
pub struct WhisperCatalog {
    pub models: Vec<WhisperModel>,
}

#[derive(Debug, Deserialize)]
pub struct ArgosCatalog {
    pub packages: Vec<ArgosPackage>,
}

pub fn whisper() -> &'static WhisperCatalog {
    static C: OnceLock<WhisperCatalog> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("../../catalog/whisper.json")).expect("valid whisper catalog"))
}

pub fn argos() -> &'static ArgosCatalog {
    static C: OnceLock<ArgosCatalog> = OnceLock::new();
    C.get_or_init(|| {
        let mut c: ArgosCatalog = serde_json::from_str(include_str!("../../catalog/argos.json")).expect("valid argos catalog");
        c.packages.retain(|p| p.supported);
        c
    })
}

pub fn whisper_model(id: &str) -> Option<&'static WhisperModel> {
    whisper().models.iter().find(|m| m.id == id)
}

pub fn argos_package(id: &str) -> Option<&'static ArgosPackage> {
    argos().packages.iter().find(|p| p.id == id)
}

pub fn argos_pair(from: &str, to: &str) -> Option<&'static ArgosPackage> {
    argos().packages.iter().find(|p| p.from == from && p.to == to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_and_are_pinned() {
        assert!(!whisper().models.is_empty());
        for m in &whisper().models {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.url.starts_with("https://"));
            assert!(!m.file.ends_with(".en.bin"), "English-only models are not allowed");
        }
        for p in &argos().packages {
            assert_eq!(p.sha256.len(), 64, "{}", p.id);
            assert_eq!(p.tokenizer, "sentencepiece");
        }
    }

    #[test]
    fn turbo_is_never_used_for_translation() {
        for m in &whisper().models {
            if m.id.contains("turbo") {
                assert!(!m.can_translate);
            }
        }
    }

    #[test]
    fn core_bangla_pairs_exist() {
        assert!(argos_pair("bn", "en").is_some());
        assert!(argos_pair("en", "bn").is_some());
    }
}
