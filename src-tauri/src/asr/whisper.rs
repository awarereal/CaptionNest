//! Runs `whisper-cli` and reads its word-level JSON output.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::Word;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::process;

#[derive(Debug, Clone)]
pub struct Options {
    pub model: PathBuf,
    /// `None` = detect automatically.
    pub language: Option<String>,
    /// Translate the speech into English.
    pub translate: bool,
    pub use_gpu: bool,
    pub vad_model: Option<PathBuf>,
    pub threads: usize,
    /// Initial prompt (see `prompt.rs`).
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub words: Vec<Word>,
    /// Language Whisper used (detected or forced).
    pub language: Option<String>,
    /// Whether GPU acceleration was actually used.
    pub used_gpu: bool,
    /// Text was not valid UTF-8 in places and had to be repaired.
    pub repaired_text: bool,
}

pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8)
}

/// Transcribes (or translates) `wav`. If the GPU run fails, retries once on the CPU.
pub async fn transcribe(
    whisper_cli: &Path,
    wav: &Path,
    out_base: &Path,
    opts: &Options,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(f32),
) -> AppResult<Transcript> {
    if !opts.model.is_file() {
        return Err(AppError::new(ErrorCode::ModelMissing, "The speech recognition model isn't installed yet."));
    }
    let first = run_once(whisper_cli, wav, out_base, opts, opts.use_gpu, cancel, &mut on_progress).await;
    match first {
        Err(e) if opts.use_gpu && !e.is_cancelled() && e.code != ErrorCode::DiskFull => {
            log::warn!("whisper GPU run failed, retrying on CPU: {e:?}");
            on_progress(0.0);
            run_once(whisper_cli, wav, out_base, opts, false, cancel, &mut on_progress).await
        }
        other => other,
    }
}

async fn run_once(
    whisper_cli: &Path,
    wav: &Path,
    out_base: &Path,
    opts: &Options,
    use_gpu: bool,
    cancel: &CancellationToken,
    on_progress: &mut impl FnMut(f32),
) -> AppResult<Transcript> {
    let json_path = out_base.with_extension("json");
    let _ = std::fs::remove_file(&json_path);

    let mut cmd = process::command(whisper_cli);
    cmd.arg("-m").arg(&opts.model)
        .arg("-f").arg(wav)
        .arg("-of").arg(out_base)
        .args(["-oj", "-ml", "1", "-sow", "-pp", "-sns"])
        .args(["-t", &opts.threads.to_string()])
        .args(["-l", opts.language.as_deref().unwrap_or("auto")]);
    if opts.translate {
        cmd.arg("-tr");
    }
    if let Some(vad) = &opts.vad_model {
        // Skip non-speech regions (reduces invented captions during silence and music).
        // whisper.cpp maps timestamps back to the original audio timeline.
        cmd.arg("--vad").arg("-vm").arg(vad)
            .args(["-vt", "0.5", "-vspd", "250", "-vsd", "300", "-vp", "200"]);
    }
    if let Some(p) = &opts.prompt {
        cmd.arg("--prompt").arg(p);
    }
    if !use_gpu {
        cmd.arg("-ng");
    }

    let mut detected = None;
    let out = process::run_streaming(
        cmd,
        cancel,
        false,
        |_| {},
        |line| {
            if let Some(p) = parse_progress(line) {
                on_progress(p);
            }
            if let Some(lang) = parse_detected_language(line) {
                detected = Some(lang);
            }
        },
    )
    .await?;

    if !out.status.success() {
        let lower = out.stderr_tail.to_ascii_lowercase();
        let err = if lower.contains("failed to load model") || lower.contains("failed to initialize whisper context") {
            AppError::new(ErrorCode::ModelMissing, "The speech recognition model couldn't be loaded. It may be damaged; remove and download it again in Settings.")
        } else if lower.contains("no space left") {
            AppError::new(ErrorCode::DiskFull, "Your disk is full. Free up some space and try again.")
        } else {
            AppError::new(ErrorCode::ToolFailed, "Speech recognition stopped unexpectedly.")
        };
        return Err(err.with_detail(out.stderr_tail));
    }

    let bytes = std::fs::read(&json_path).map_err(|e| {
        AppError::new(ErrorCode::ToolFailed, "Speech recognition finished without results.").with_detail(e.to_string())
    })?;
    let _ = std::fs::remove_file(&json_path);
    let mut t = parse_json(&bytes)?;
    if t.language.is_none() {
        t.language = detected.or_else(|| opts.language.clone());
    }
    t.used_gpu = use_gpu;
    on_progress(1.0);
    Ok(t)
}

/// Detects the spoken language from the first 30 seconds of speech (one encoder pass).
/// Returns the language and Whisper's probability, or None when detection failed (the
/// main pass then detects on its own).
pub async fn detect_language(
    whisper_cli: &Path,
    wav: &Path,
    opts: &Options,
    cancel: &CancellationToken,
) -> AppResult<Option<(String, f32)>> {
    let run = |gpu: bool| {
        let mut cmd = process::command(whisper_cli);
        cmd.arg("-m").arg(&opts.model).arg("-f").arg(wav).args(["-dl", "-l", "auto"])
            .args(["-t", &opts.threads.to_string()]);
        if let Some(vad) = &opts.vad_model {
            cmd.arg("--vad").arg("-vm").arg(vad);
        }
        if !gpu {
            cmd.arg("-ng");
        }
        cmd
    };
    let mut found = None;
    let out = process::run_streaming(run(opts.use_gpu), cancel, false, |_| {}, |l| {
        if let Some(d) = parse_detection(l) {
            found = Some(d);
        }
    })
    .await?;
    if found.is_none() && !out.status.success() && opts.use_gpu {
        process::run_streaming(run(false), cancel, false, |_| {}, |l| {
            if let Some(d) = parse_detection(l) {
                found = Some(d);
            }
        })
        .await?;
    }
    Ok(found)
}

fn parse_progress(line: &str) -> Option<f32> {
    let rest = line.split("progress =").nth(1)?;
    let n: f32 = rest.trim().trim_end_matches('%').trim().parse().ok()?;
    Some((n / 100.0).clamp(0.0, 1.0))
}

/// "auto-detected language: bn (p = 0.912)" → ("bn", 0.912)
fn parse_detection(line: &str) -> Option<(String, f32)> {
    let lang = parse_detected_language(line)?;
    let p = line.split("p =").nth(1).and_then(|r| r.trim().trim_end_matches(')').trim().parse().ok()).unwrap_or(0.0);
    Some((lang, p))
}

fn parse_detected_language(line: &str) -> Option<String> {
    let rest = line.split("auto-detected language:").nth(1)?;
    let code = rest.split_whitespace().next()?;
    (!code.is_empty()).then(|| code.to_string())
}

#[derive(Deserialize)]
struct RawOutput {
    #[serde(default)]
    result: Option<RawResult>,
    #[serde(default)]
    transcription: Vec<RawSegment>,
}

#[derive(Deserialize)]
struct RawResult {
    language: Option<String>,
}

#[derive(Deserialize)]
struct RawSegment {
    offsets: RawOffsets,
    text: String,
}

#[derive(Deserialize)]
struct RawOffsets {
    from: i64,
    to: i64,
}

/// Parses whisper-cli JSON. Whisper's byte-level tokens can split a multi-byte character
/// (common with Bengali, Telugu and other scripts on small models), which makes the file
/// invalid UTF-8; those fragments are dropped instead of failing the whole job.
pub(crate) fn parse_json(bytes: &[u8]) -> AppResult<Transcript> {
    let (text, repaired) = match std::str::from_utf8(bytes) {
        Ok(s) => (std::borrow::Cow::Borrowed(s), false),
        Err(_) => (String::from_utf8_lossy(bytes), true),
    };
    let raw: RawOutput = serde_json::from_str(&text).map_err(|e| {
        AppError::new(ErrorCode::ToolFailed, "Speech recognition produced unreadable results.").with_detail(e.to_string())
    })?;
    let words = raw
        .transcription
        .into_iter()
        .filter_map(|s| {
            let cleaned: String = s.text.chars().filter(|c| *c != '\u{FFFD}').collect();
            if cleaned.trim().is_empty() {
                return None;
            }
            Some(Word { start_ms: s.offsets.from.max(0), end_ms: s.offsets.to.max(s.offsets.from), text: cleaned })
        })
        .collect();
    Ok(Transcript {
        words,
        language: raw.result.and_then(|r| r.language),
        used_gpu: false,
        repaired_text: repaired,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_bangla_word_output() {
        let t = parse_json(include_bytes!("../../tests/fixtures/whisper_bn_words.json")).unwrap();
        assert_eq!(t.language.as_deref(), Some("bn"));
        assert!(!t.repaired_text);
        assert_eq!(t.words.len(), 19);
        assert_eq!(t.words[0].text, " আমার");
        assert_eq!((t.words[0].start_ms, t.words[0].end_ms), (0, 360));
        assert!(t.words.last().unwrap().text.ends_with('।'));
        assert!(t.words.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
    }

    #[test]
    #[allow(invalid_from_utf8)]
    fn survives_invalid_utf8_from_split_tokens() {
        let bytes = include_bytes!("../../tests/fixtures/whisper_broken_utf8.json");
        assert!(std::str::from_utf8(bytes).is_err(), "fixture must contain invalid UTF-8");
        let t = parse_json(bytes).unwrap();
        assert!(t.repaired_text);
        assert!(t.words.iter().all(|w| !w.text.contains('\u{FFFD}')));
        assert!(!t.words.is_empty());
    }

    #[test]
    fn reads_progress_and_language_lines() {
        assert_eq!(parse_progress("whisper_print_progress_callback: progress =  45%"), Some(0.45));
        assert_eq!(parse_progress("whisper_print_progress_callback: progress = 100%"), Some(1.0));
        assert_eq!(parse_progress("unrelated"), None);
        assert_eq!(parse_detected_language("whisper_full_with_state: auto-detected language: bn (p = 0.912)").as_deref(), Some("bn"));
        let (l, p) = parse_detection("whisper_full_with_state: auto-detected language: gu (p = 0.210000)").unwrap();
        assert_eq!(l, "gu");
        assert!((p - 0.21).abs() < 1e-6);
    }

    #[test]
    fn empty_transcription_means_no_words() {
        let t = parse_json(br#"{"result":{"language":"en"},"transcription":[]}"#).unwrap();
        assert!(t.words.is_empty());
    }
}
