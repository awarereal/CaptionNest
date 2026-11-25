//! Chooses how to produce captions in the requested language.
//!
//! - Same language as the speech: no translation.
//! - English: Whisper's built-in speech-to-English translation (needs a model that
//!   supports translation).
//! - Any other language: Argos packages applied to the transcript text, so every
//!   translated cue keeps the original cue's timing exactly. Argos pivots through
//!   English when there is no direct package. If the spoken language has no Argos
//!   package at all, Whisper's English translation is used as the pivot instead.

use std::collections::HashSet;

use serde::Serialize;

use super::argos_code;
use crate::models::catalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Plan {
    /// Captions stay in the spoken language.
    Same,
    /// Whisper translates speech to English.
    WhisperEnglish,
    /// Argos packages applied in order to a text track.
    Text {
        /// Translate Whisper's English track instead of the transcript.
        from_whisper_english: bool,
        packages: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    pub plan: Option<Plan>,
    /// Packages the plan needs that aren't installed yet.
    pub missing_packages: Vec<String>,
    pub needs_translation_model: bool,
}

/// `source` is the spoken language (Whisper code), `target` the caption language.
pub fn plan(source: &str, target: &str) -> Option<Plan> {
    if target == source || target == "same" {
        return Some(Plan::Same);
    }
    if target == "en" {
        return Some(Plan::WhisperEnglish);
    }
    let (s, t) = (argos_code(source), argos_code(target));
    let pair = |a: &str, b: &str| catalog::argos_pair(a, b).map(|p| p.id.clone());
    if let Some(direct) = pair(s, t) {
        return Some(Plan::Text { from_whisper_english: false, packages: vec![direct] });
    }
    let en_t = pair("en", t)?;
    if s == "en" {
        return Some(Plan::Text { from_whisper_english: false, packages: vec![en_t] });
    }
    if let Some(s_en) = pair(s, "en") {
        return Some(Plan::Text { from_whisper_english: false, packages: vec![s_en, en_t] });
    }
    Some(Plan::Text { from_whisper_english: true, packages: vec![en_t] })
}

pub fn describe(source: &str, target: &str, installed: &HashSet<String>) -> PlanInfo {
    let plan = plan(source, target);
    let (missing_packages, needs_translation_model) = match &plan {
        Some(Plan::Text { packages, from_whisper_english }) => (
            packages.iter().filter(|p| !installed.contains(*p)).cloned().collect(),
            *from_whisper_english,
        ),
        Some(Plan::WhisperEnglish) => (vec![], true),
        _ => (vec![], false),
    };
    PlanInfo { plan, missing_packages, needs_translation_model }
}

/// Every language code captions can be produced in through Argos (target side).
pub fn argos_targets() -> Vec<String> {
    let mut v: Vec<String> = catalog::argos().packages.iter().filter(|p| p.from == "en").map(|p| p.to.clone()).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_language_needs_nothing() {
        assert_eq!(plan("bn", "bn"), Some(Plan::Same));
        assert_eq!(plan("bn", "same"), Some(Plan::Same));
    }

    #[test]
    fn english_uses_whisper_translation() {
        assert_eq!(plan("bn", "en"), Some(Plan::WhisperEnglish));
        assert_eq!(plan("hi", "en"), Some(Plan::WhisperEnglish));
    }

    #[test]
    fn english_speech_to_bangla_uses_one_package() {
        assert_eq!(plan("en", "bn"), Some(Plan::Text { from_whisper_english: false, packages: vec!["en_bn".into()] }));
    }

    #[test]
    fn bangla_to_hindi_pivots_through_english_text() {
        if catalog::argos_pair("hi", "en").is_none() || catalog::argos_pair("en", "hi").is_none() {
            return;
        }
        assert_eq!(
            plan("bn", "hi"),
            Some(Plan::Text { from_whisper_english: false, packages: vec!["bn_en".into(), "en_hi".into()] })
        );
    }

    #[test]
    fn unsupported_target_has_no_plan() {
        assert_eq!(plan("bn", "xx"), None);
    }

    #[test]
    fn missing_packages_are_listed() {
        let info = describe("en", "bn", &HashSet::new());
        assert_eq!(info.missing_packages, vec!["en_bn".to_string()]);
        let installed: HashSet<String> = ["en_bn".to_string()].into();
        assert!(describe("en", "bn", &installed).missing_packages.is_empty());
    }
}
