//! Runs one installed Argos Translate package.
//!
//! Mirrors the reference `argostranslate` pipeline for SentencePiece packages:
//! SentencePiece pieces in, CTranslate2 beam search (beam 4, length penalty 0.2,
//! unknown-token replacement), pieces joined with "▁" turned back into spaces.

use std::path::Path;

use ct2rs::{Config, Device, TranslationOptions, Translator};
use sentencepiece::SentencePieceProcessor;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};

pub struct ArgosTokenizer {
    sp: SentencePieceProcessor,
}

impl ct2rs::Tokenizer for ArgosTokenizer {
    fn encode(&self, input: &str) -> anyhow::Result<Vec<String>> {
        Ok(self.sp.encode(input)?.into_iter().map(|p| p.piece).collect())
    }

    fn decode(&self, tokens: Vec<String>) -> anyhow::Result<String> {
        Ok(detokenize(&tokens))
    }
}

/// Joins SentencePiece pieces the way Argos does.
pub fn detokenize(tokens: &[String]) -> String {
    let joined: String = tokens.concat().replace('▁', " ");
    joined.trim().to_string()
}

pub struct PackageTranslator {
    inner: Translator<ArgosTokenizer>,
}

impl PackageTranslator {
    /// Loads a package folder that contains `model/` and `sentencepiece.model`.
    pub fn load(dir: &Path, threads: usize) -> AppResult<Self> {
        let sp = SentencePieceProcessor::open(dir.join("sentencepiece.model")).map_err(|e| {
            AppError::new(ErrorCode::TranslationFailed, "A translation package is damaged. Remove it and download it again in Settings.")
                .with_detail(e.to_string())
        })?;
        let config = Config {
            device: Device::CPU,
            num_threads_per_replica: threads,
            ..Default::default()
        };
        let inner = Translator::with_tokenizer(dir.join("model"), ArgosTokenizer { sp }, &config).map_err(|e| {
            AppError::new(ErrorCode::TranslationFailed, "A translation package couldn't be loaded.").with_detail(e.to_string())
        })?;
        Ok(Self { inner })
    }

    /// Translates each input independently (one caption cue each), in small batches so
    /// progress can be reported and cancellation is quick.
    pub fn translate_all(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(usize, usize),
    ) -> AppResult<Vec<String>> {
        let options = TranslationOptions::<String, String> {
            beam_size: 4,
            length_penalty: 0.2,
            replace_unknowns: true,
            max_batch_size: 16,
            ..Default::default()
        };
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(8) {
            if cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            // Empty inputs stay empty; CTranslate2 rejects empty token lists.
            let (idx, inputs): (Vec<usize>, Vec<String>) = chunk
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.trim().is_empty())
                .map(|(i, t)| (i, t.replace('\n', " ")))
                .unzip();
            let mut results = vec![String::new(); chunk.len()];
            if !inputs.is_empty() {
                let translated = self.inner.translate_batch(&inputs, &options, None).map_err(|e| {
                    AppError::new(ErrorCode::TranslationFailed, "Translation stopped unexpectedly.").with_detail(e.to_string())
                })?;
                for (i, (text, _score)) in idx.into_iter().zip(translated) {
                    results[i] = text;
                }
            }
            out.extend(results);
            on_progress(out.len(), texts.len());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detokenizes_like_argos() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(detokenize(&t(&["▁Hello", ",", "▁world", "!"])), "Hello, world!");
        assert_eq!(detokenize(&t(&["▁আমি", "▁বাংলায়", "▁কথা", "▁বলি", "।"])), "আমি বাংলায় কথা বলি।");
        assert_eq!(detokenize(&[]), "");
    }

    /// Runs against a real installed package when CAPTIONNEST_ARGOS_DIR points at one,
    /// e.g. `.../translation/bn_en`. Compared against reference argostranslate output in
    /// tests/fixtures/argos_reference.json.
    #[test]
    #[ignore]
    fn matches_reference_argostranslate_output() {
        let Ok(root) = std::env::var("CAPTIONNEST_ARGOS_ROOT") else { return };
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/argos_reference.json")).unwrap();
        let mut checked = 0;
        for case in fixtures["cases"].as_array().unwrap() {
            let pkg = case["package"].as_str().unwrap();
            let dir = std::path::Path::new(&root).join(pkg);
            if !dir.exists() {
                continue;
            }
            let tr = PackageTranslator::load(&dir, 4).unwrap();
            let input = case["input"].as_str().unwrap().to_string();
            let got = tr.translate_all(&[input.clone()], &CancellationToken::new(), |_, _| {}).unwrap();
            assert_eq!(got[0], case["expected"].as_str().unwrap(), "package {pkg}, input {input}");
            checked += 1;
        }
        assert!(checked > 0, "no packages found under {root}");
    }
}
