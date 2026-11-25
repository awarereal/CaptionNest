//! Runs `whisper-cli` on speech chunks and reads its word-level JSON output.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::Word;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::{process, wav};

#[derive(Debug, Clone)]
pub struct Options {
    pub model: PathBuf,
    /// `None` = detect automatically.
    pub language: Option<String>,
    /// Translate the speech into English.
    pub translate: bool,
    pub use_gpu: bool,
    pub threads: usize,
    /// Word-level timing (-ml 1 -sow). Off for translation, whose word times don't
    /// follow the audio; whole sentences are used instead.
    pub word_timestamps: bool,
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

/// Files per whisper-cli run. One run loads the model once and processes every file;
/// batching keeps command lines short (Windows limits them to 32,767 characters).
const BATCH: usize = 200;

/// How many recognizer processes run side by side. Two processes keep the GPU (or the
/// processor's cores) busier; on CaptionNest's tests this was about 20% faster.
pub fn parallel_processes(use_gpu: bool) -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    if (use_gpu && cfg!(all(target_os = "macos", target_arch = "aarch64"))) || cores >= 8 { 2 } else { 1 }
}

/// Encoder window (1500 frames = 30 s) just large enough for the longest chunk. Whisper
/// otherwise encodes a full 30 s for every chunk; matching the window to 12-second chunks
/// was 22% faster with equal or better accuracy in CaptionNest's Bangla test.
pub fn audio_ctx_for(max_chunk_ms: i64) -> Option<u32> {
    if max_chunk_ms >= 28_000 {
        return None;
    }
    let frames = ((max_chunk_ms.max(1_000) as f64 / 30_000.0) * 1500.0 / 64.0).ceil() as u32 * 64 + 64;
    Some(frames.min(1500))
}

/// Recognizes the given spans of `wav` (each at most ~30 s), returning words on the
/// original timeline. Each span is decoded on its own, so a misrecognition can't spread
/// across a long video, and no stretch of speech is skipped by the decoder jumping
/// ahead. If the GPU fails, the remaining work continues on the processor.
pub async fn transcribe_spans(
    whisper_cli: &Path,
    wav: &Path,
    spans: &[(i64, i64)],
    work_dir: &Path,
    opts: &Options,
    cancel: &CancellationToken,
    on_progress: impl FnMut(f32) + Send,
) -> AppResult<Transcript> {
    if !opts.model.is_file() {
        return Err(AppError::new(ErrorCode::ModelMissing, "The speech recognition model isn't installed yet."));
    }
    std::fs::create_dir_all(work_dir)?;
    let info = wav::read_info(wav).map_err(|e| AppError::new(ErrorCode::ToolFailed, "The prepared audio couldn't be read.").with_detail(e.to_string()))?;
    let total: i64 = spans.iter().map(|(s, e)| (e - s).max(1)).sum::<i64>().max(1);

    // Contiguous groups, one recognizer process each.
    let n = if spans.len() >= 8 { parallel_processes(opts.use_gpu) } else { 1 };
    let groups: Vec<&[(i64, i64)]> = spans.chunks(spans.len().div_ceil(n).max(1)).collect();
    let mut gopts = opts.clone();
    gopts.threads = (opts.threads / groups.len()).max(2);

    let done = std::sync::Mutex::new(vec![0i64; groups.len()]);
    let on_progress = std::sync::Mutex::new(on_progress);
    let report = |g: usize, ms: i64| {
        let sum: i64 = {
            let mut d = done.lock().unwrap();
            d[g] = ms;
            d.iter().sum()
        };
        (on_progress.lock().unwrap())((sum as f64 / total as f64).clamp(0.0, 1.0) as f32);
    };
    let report = &report;
    let runs = groups.iter().enumerate().map(|(g, group)| {
        let dir = work_dir.join(format!("g{g}"));
        let gopts = &gopts;
        let info = &info;
        async move { run_group(whisper_cli, wav, info, group, &dir, gopts, cancel, move |ms| report(g, ms)).await }
    });
    let results = futures_util::future::try_join_all(runs).await?;

    let mut out = Transcript { words: Vec::new(), language: opts.language.clone(), used_gpu: opts.use_gpu, repaired_text: false };
    for t in results {
        out.used_gpu &= t.used_gpu;
        out.repaired_text |= t.repaired_text;
        if out.language.is_none() {
            out.language = t.language;
        }
        out.words.extend(t.words);
    }
    out.words.sort_by_key(|w| w.start_ms);
    (on_progress.lock().unwrap())(1.0);
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn run_group(
    whisper_cli: &Path,
    wav: &Path,
    info: &wav::WavInfo,
    spans: &[(i64, i64)],
    work_dir: &Path,
    opts: &Options,
    cancel: &CancellationToken,
    report: impl Fn(i64),
) -> AppResult<Transcript> {
    std::fs::create_dir_all(work_dir)?;
    let mut done_ms = 0i64;
    let mut use_gpu = opts.use_gpu;
    let mut out = Transcript { words: Vec::new(), language: opts.language.clone(), used_gpu: use_gpu, repaired_text: false };

    for (batch_no, batch) in spans.chunks(BATCH).enumerate() {
        let names: Vec<String> = (0..batch.len()).map(|i| format!("c{:03}-{:05}.wav", batch_no, i)).collect();
        for (name, &(s, e)) in names.iter().zip(batch) {
            wav::write_slice(wav, info, s, e, &work_dir.join(name))?;
        }
        let durations: Vec<i64> = batch.iter().map(|(s, e)| (e - s).max(1)).collect();
        let audio_ctx = audio_ctx_for(durations.iter().copied().max().unwrap_or(30_000));
        let result = run_batch(whisper_cli, work_dir, &names, &durations, opts, audio_ctx, use_gpu, cancel, |p| report(done_ms + p)).await;
        match result {
            Ok(()) => {}
            Err(e) if use_gpu && !e.is_cancelled() && e.code != ErrorCode::DiskFull => {
                log::warn!("whisper GPU run failed, continuing on the processor: {e:?}");
                use_gpu = false;
                out.used_gpu = false;
                run_batch(whisper_cli, work_dir, &names, &durations, opts, audio_ctx, false, cancel, |p| report(done_ms + p)).await?;
            }
            Err(e) => return Err(e),
        }
        for (name, &(s, _)) in names.iter().zip(batch) {
            let json = work_dir.join(format!("{name}.json"));
            let bytes = std::fs::read(&json).map_err(|e| {
                AppError::new(ErrorCode::ToolFailed, "Speech recognition finished without results.").with_detail(format!("{name}: {e}"))
            })?;
            let mut t = parse_json(&bytes)?;
            if let Some(p) = &opts.prompt {
                t.words = super::prompt::strip_echo(t.words, p);
            }
            out.repaired_text |= t.repaired_text;
            if out.language.is_none() {
                out.language = t.language;
            }
            out.words.extend(t.words.into_iter().map(|w| Word { start_ms: w.start_ms + s, end_ms: w.end_ms + s, ..w }));
            let _ = std::fs::remove_file(&json);
            let _ = std::fs::remove_file(work_dir.join(name));
        }
        done_ms += durations.iter().sum::<i64>();
        report(done_ms);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn run_batch(
    whisper_cli: &Path,
    work_dir: &Path,
    names: &[String],
    durations: &[i64],
    opts: &Options,
    audio_ctx: Option<u32>,
    use_gpu: bool,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(i64),
) -> AppResult<()> {
    let mut cmd = process::command(whisper_cli);
    // Short relative file names keep the command line small.
    cmd.current_dir(work_dir);
    cmd.arg("-m").arg(&opts.model)
        .args(["-ojf", "-pp", "-sns"])
        .args(["-t", &opts.threads.to_string()])
        .args(["-l", opts.language.as_deref().unwrap_or("auto")]);
    if opts.word_timestamps {
        cmd.args(["-ml", "1", "-sow"]);
        // Token alignment from the attention weights (DTW) places words within a few
        // hundredths of a second, where Whisper's own word times drift by 0.3-0.6 s at
        // the edges of speech. DTW is skipped when flash attention is on, so turn that off.
        if let Some(preset) = dtw_preset(&opts.model) {
            cmd.args(["-dtw", preset, "-nfa"]);
        }
    }
    if opts.translate {
        cmd.arg("-tr");
    }
    if let Some(p) = &opts.prompt {
        cmd.arg("--prompt").arg(p);
    }
    if let Some(ac) = audio_ctx {
        cmd.args(["-ac", &ac.to_string()]);
    }
    if !use_gpu {
        cmd.arg("-ng");
    }
    for n in names {
        cmd.arg("-f").arg(n);
    }

    // Progress: whisper-cli announces each file, then reports 0-100% within it.
    let mut file_idx: Option<usize> = None;
    let mut before: i64 = 0;
    let out = process::run_streaming(cmd, cancel, false, |_| {}, |line| {
        if line.contains(": processing '") {
            if let Some(i) = file_idx {
                before += durations.get(i).copied().unwrap_or(0);
            }
            let i = file_idx.map_or(0, |i| i + 1);
            file_idx = Some(i);
            on_progress(before);
        } else if let Some(p) = parse_progress(line) {
            let cur = file_idx.and_then(|i| durations.get(i)).copied().unwrap_or(0);
            on_progress(before + (cur as f32 * p) as i64);
        }
    })
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
    Ok(())
}

/// whisper.cpp's alignment-head preset for a model file, from its name. Fine-tuned
/// models (e.g. the Bangla ones) share the architecture, and so the heads, of their base.
pub fn dtw_preset(model: &Path) -> Option<&'static str> {
    let name = model.file_name()?.to_string_lossy().to_ascii_lowercase();
    let presets = [
        ("large-v3-turbo", "large.v3.turbo"),
        ("large-v3", "large.v3"),
        ("large-v2", "large.v2"),
        ("large-v1", "large.v1"),
        ("medium.en", "medium.en"),
        ("medium", "medium"),
        ("small.en", "small.en"),
        ("small", "small"),
        ("base.en", "base.en"),
        ("base", "base"),
        ("tiny.en", "tiny.en"),
        ("tiny", "tiny"),
    ];
    presets.iter().find(|(key, _)| name.contains(key)).map(|(_, p)| *p)
}

/// Splits `[start, end)` into windows of at most `max_ms` (used when no voice activity
/// information is available).
pub fn fixed_windows(start: i64, end: i64, max_ms: i64) -> Vec<(i64, i64)> {
    let mut v = Vec::new();
    let mut s = start;
    while s < end {
        let e = (s + max_ms).min(end);
        v.push((s, e));
        s = e;
    }
    v
}

/// Detects the spoken language from the first 30 seconds of speech (one encoder pass).
/// Returns the language and Whisper's probability, or None when detection failed (the
/// main pass then detects on its own).
pub async fn detect_language(
    whisper_cli: &Path,
    wav: &Path,
    opts: &Options,
    start_ms: i64,
    cancel: &CancellationToken,
) -> AppResult<Option<(String, f32)>> {
    let run = |gpu: bool| {
        let mut cmd = process::command(whisper_cli);
        cmd.arg("-m").arg(&opts.model).arg("-f").arg(wav).args(["-dl", "-l", "auto"])
            .args(["-t", &opts.threads.to_string()]);
        if start_ms > 0 {
            // Listen from the first speech, not from leading music or silence.
            cmd.args(["-ot", &start_ms.to_string()]);
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
    #[serde(default)]
    tokens: Vec<RawToken>,
}

#[derive(Deserialize)]
struct RawToken {
    #[serde(default)]
    text: String,
    #[serde(default = "one")]
    p: f32,
    /// DTW-aligned time of the token in centiseconds; -1 when not computed.
    #[serde(default = "minus_one")]
    t_dtw: i64,
}

fn one() -> f32 {
    1.0
}

fn minus_one() -> i64 {
    -1
}

/// DTW start of a word: the aligned time of its first token that carries text.
fn dtw_start_ms(tokens: &[RawToken], from: i64, to: i64) -> Option<i64> {
    let t = tokens.iter().find(|t| !t.text.starts_with("[_") && !t.text.trim().is_empty())?;
    let ms = t.t_dtw * 10;
    // Ignore alignments that land far outside Whisper's own estimate for the word.
    (t.t_dtw >= 0 && ms >= from - 1_000 && ms <= to + 500).then_some(ms)
}

/// Lowest probability among the tokens that carry letters (special tokens such as
/// "[_BEG_]", spaces and punctuation don't count).
fn word_confidence(tokens: &[RawToken]) -> f32 {
    tokens
        .iter()
        .filter(|t| !t.text.starts_with("[_") && t.text.chars().any(|c| c.is_alphanumeric() || c == '\u{FFFD}' || ('\u{0980}'..='\u{09FF}').contains(&c)))
        .map(|t| t.p)
        .fold(1.0_f32, f32::min)
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
    let mut dtw: Vec<Option<i64>> = Vec::new();
    let mut words: Vec<Word> = raw
        .transcription
        .into_iter()
        .filter_map(|s| {
            let cleaned: String = s.text.chars().filter(|c| *c != '\u{FFFD}').collect();
            if cleaned.trim().is_empty() {
                return None;
            }
            dtw.push(dtw_start_ms(&s.tokens, s.offsets.from, s.offsets.to));
            Some(Word { confidence: word_confidence(&s.tokens), start_ms: s.offsets.from.max(0), end_ms: s.offsets.to.max(s.offsets.from), text: cleaned })
        })
        .collect();
    apply_dtw(&mut words, &dtw);
    Ok(Transcript {
        words,
        language: raw.result.and_then(|r| r.language),
        used_gpu: false,
        repaired_text: repaired,
    })
}

/// Moves word starts to their DTW-aligned times. A word then ends where the next one
/// starts, or earlier if Whisper heard it end earlier (a pause).
fn apply_dtw(words: &mut [Word], dtw: &[Option<i64>]) {
    if dtw.iter().all(Option::is_none) {
        return;
    }
    let mut prev = 0;
    for (w, d) in words.iter_mut().zip(dtw) {
        if let Some(ms) = d {
            w.start_ms = (*ms).max(prev).max(0);
        }
        prev = w.start_ms;
    }
    for i in 0..words.len() {
        let next = words.get(i + 1).map(|n| n.start_ms);
        let w = &mut words[i];
        let heard_end = w.end_ms;
        w.end_ms = match next {
            Some(n) if heard_end > w.start_ms + 30 => heard_end.min(n),
            Some(n) => n,
            None => heard_end,
        }
        .max(w.start_ms + 30);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtw_moves_word_starts_and_keeps_pauses() {
        let json = br#"{"result":{"language":"bn"},"transcription":[
          {"offsets":{"from":0,"to":490},"text":"a","tokens":[{"text":"[_BEG_]","p":1,"t_dtw":-1},{"text":"a","p":0.9,"t_dtw":78}]},
          {"offsets":{"from":490,"to":1280},"text":" bb","tokens":[{"text":" ","p":1,"t_dtw":120},{"text":"bb","p":0.9,"t_dtw":120}]},
          {"offsets":{"from":1280,"to":1500},"text":" c","tokens":[{"text":" c","p":0.9,"t_dtw":190}]},
          {"offsets":{"from":3000,"to":3600},"text":" d","tokens":[{"text":" d","p":0.9,"t_dtw":320}]}]}"#;
        let t = parse_json(json).unwrap();
        let s: Vec<(i64, i64)> = t.words.iter().map(|w| (w.start_ms, w.end_ms)).collect();
        // Starts come from DTW. A word ends where the next starts unless Whisper heard it end
        // earlier: "bb" ends at 1280. "c" was heard ending at 1500, before its DTW start,
        // so it runs to the next word.
        assert_eq!(s, vec![(780, 1200), (1200, 1280), (1900, 3200), (3200, 3600)]);
    }

    #[test]
    fn dtw_presets_follow_the_model_file() {
        assert_eq!(dtw_preset(Path::new("/m/ggml-large-v3-turbo-bn-q5_0.bin")), Some("large.v3.turbo"));
        assert_eq!(dtw_preset(Path::new("/m/ggml-large-v3-q5_0.bin")), Some("large.v3"));
        assert_eq!(dtw_preset(Path::new("/m/ggml-small.bin")), Some("small"));
        assert_eq!(dtw_preset(Path::new("/m/custom.bin")), None);
    }

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
    fn audio_window_fits_the_longest_chunk() {
        assert_eq!(audio_ctx_for(12_500), Some(704));
        assert!(audio_ctx_for(13_600).unwrap() >= 13_600 * 1500 / 30_000);
        assert_eq!(audio_ctx_for(29_000), None);
        assert!(audio_ctx_for(100).unwrap() >= 64);
    }

    #[test]
    fn fixed_windows_cover_the_range() {
        assert_eq!(fixed_windows(0, 25_000, 12_000), vec![(0, 12_000), (12_000, 24_000), (24_000, 25_000)]);
        assert!(fixed_windows(5, 5, 1_000).is_empty());
    }

    #[test]
    fn reads_word_confidence_from_tokens() {
        let json = r#"{"transcription":[
          {"offsets":{"from":0,"to":500},"text":" বলি।","tokens":[
            {"text":"[_BEG_]","p":0.2},{"text":" ","p":0.99},{"text":"ব","p":0.66},{"text":"ল","p":0.83},{"text":"।","p":0.39}]},
          {"offsets":{"from":500,"to":900},"text":" নাম","tokens":[{"text":"ন","p":0.99},{"text":"ম","p":0.97}]}]}"#;
        let t = parse_json(json.as_bytes()).unwrap();
        assert!((t.words[0].confidence - 0.66).abs() < 1e-6, "punctuation and special tokens ignored");
        assert!((t.words[1].confidence - 0.97).abs() < 1e-6);
    }

    #[test]
    fn empty_transcription_means_no_words() {
        let t = parse_json(br#"{"result":{"language":"en"},"transcription":[]}"#).unwrap();
        assert!(t.words.is_empty());
    }
}
