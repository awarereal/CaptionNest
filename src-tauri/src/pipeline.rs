//! The caption generation pipeline:
//! prepare → extract audio → transcribe → (translate) → readable cues.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::asr::{align, prompt, sarvam, script, segment, vad, whisper};
use crate::captions::Cue;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::{Reporter, ScratchDir};
use crate::media::{extract, probe};
use crate::models::{self, catalog};
use crate::paths::Paths;
use crate::project::{Track, TrackKind};
use crate::translate::{argos::PackageTranslator, routes};

/// Model used for automatic language detection when installed.
const DETECTION_MODEL: &str = "large-v3-q5_0";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub video_path: String,
    pub audio_stream_index: u32,
    pub duration_ms: i64,
    /// "auto" or a Whisper language code.
    pub spoken_language: String,
    /// "same" or a language code.
    pub caption_language: String,
    pub transcribe_model_id: String,
    pub translate_model_id: Option<String>,
    pub use_gpu: bool,
    pub max_line_chars: Option<usize>,
    /// Use a language-specialised model (e.g. Bangla) when one is installed. Default on.
    #[serde(default)]
    pub use_language_models: Option<bool>,
    /// Testing only: use this model file instead of a catalog model.
    #[serde(default)]
    pub model_path_override: Option<String>,
    /// Online recognition (filled in from Settings by the app, never sent by the UI).
    #[serde(skip)]
    pub online: Option<sarvam::Online>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateResult {
    pub tracks: Vec<Track>,
    pub active_track_id: String,
    pub detected_language: Option<String>,
    pub used_gpu: bool,
    pub warnings: Vec<String>,
    /// The language-specialised model used, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub video_path: String,
    pub audio_stream_index: u32,
    pub duration_ms: i64,
    pub source_track: Track,
    pub target_language: String,
    pub translate_model_id: Option<String>,
    pub use_gpu: bool,
    pub max_line_chars: Option<usize>,
}

struct Ctx<'a> {
    paths: &'a Paths,
    cancel: &'a CancellationToken,
    report: &'a Reporter,
    scratch: &'a ScratchDir,
    seg: segment::Options,
    /// Speech regions found by voice activity detection (empty if unknown).
    speech: vad::Segments,
}

pub async fn generate(paths: &Paths, req: GenerateRequest, job_id: &str, cancel: &CancellationToken, report: &Reporter) -> AppResult<GenerateResult> {
    report.stage("preparing", None, None);
    let video = PathBuf::from(&req.video_path);
    if !video.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound, "The video can't be found. It may have been moved or deleted."));
    }
    let spoken = (req.spoken_language != "auto").then(|| req.spoken_language.clone());
    let online = req.online.clone();
    // Spoken languages Sarvam doesn't know still need an offline model.
    let online_language_ok = spoken.as_deref().is_none_or(|l| sarvam::language_code(l).is_some());
    let offline_model = match &req.model_path_override {
        Some(p) => Ok(PathBuf::from(p)),
        None => installed_whisper(paths, &req.transcribe_model_id),
    };
    if online.is_none() || !online_language_ok {
        if let Err(e) = &offline_model {
            return Err(e.clone());
        }
    }
    let transcribe_model = offline_model.clone().unwrap_or_default();
    // Fail fast, before minutes of work, when a needed model or package is missing.
    preflight_translation(paths, spoken.as_deref(), &req.caption_language, &req.transcribe_model_id, req.translate_model_id.as_deref())?;

    let scratch = ScratchDir::create(&paths.jobs, job_id)?;
    let mut seg = segment::Options::default();
    if let Some(n) = req.max_line_chars {
        seg.max_line_chars = n.clamp(16, 80);
    }
    let ctx = Ctx { paths, cancel, report, scratch: &scratch, seg, speech: Vec::new() };

    let wav = extract_wav(&ctx, &video, req.audio_stream_index, req.duration_ms).await?;

    report.stage("transcribing", None, Some("listening".into()));
    let speech = find_speech(&ctx, &wav).await?;
    let first_speech = speech.first().map(|s| (s.0 - 300).max(0)).unwrap_or(0);

    let mut opts = whisper::Options {
        model: transcribe_model,
        language: spoken.clone(),
        translate: false,
        use_gpu: req.use_gpu,
        threads: whisper::default_threads(),
        word_timestamps: true,
        prompt: None,
        timing_only: false,
    };
    let mut uncertain = None;
    let can_detect = offline_model.is_ok() || catalog::whisper_model(DETECTION_MODEL).is_some_and(|m| models::whisper_installed(paths, m));
    if opts.language.is_none() && can_detect {
        // The Accurate model identifies languages far more reliably (on real Bangla speech
        // it was right 20 of 20 times; the Fast model 3 of 20), so it detects when present.
        let mut detect_opts = opts.clone();
        if let Some(m) = catalog::whisper_model(DETECTION_MODEL).filter(|m| models::whisper_installed(paths, m)) {
            detect_opts.model = models::whisper_path(paths, m);
        }
        // Listen at up to three places (start, middle, later) and vote, so a music
        // intro or a short greeting in another language can't decide on its own.
        let points = detection_points(&speech, first_speech);
        let votes = whisper::detect_languages(&paths.tool("whisper-cli")?, &wav, &detect_opts, &points, &scratch.path().join("detect"), cancel).await?;
        if let Some((lang, confidence)) = vote_language(&votes) {
            if confidence < 0.5 {
                uncertain = Some(format!("language_uncertain:{lang}:{confidence:.2}"));
            }
            opts.language = Some(lang);
        }
    }
    // A model trained for this language (e.g. Bangla) spells it far better than the
    // general model; use it when installed and allowed.
    let mut specialised: Option<String> = None;
    let mut specialised_prompt = false;
    if req.use_language_models.unwrap_or(true) && req.model_path_override.is_none() {
        if let Some(lang) = opts.language.as_deref() {
            if let Some(m) = catalog::language_models(lang).find(|m| models::whisper_installed(paths, m)) {
                opts.model = models::whisper_path(paths, m);
                specialised = Some(m.id.clone());
                specialised_prompt = m.prompt;
            }
        }
    }
    opts.prompt = if specialised.is_some() && !specialised_prompt {
        None
    } else {
        opts.language.as_deref().and_then(prompt::initial_prompt).map(str::to_string)
    };
    let mut seg = ctx.seg.clone();
    seg.speech = vad::unpadded(&speech);
    let ctx = Ctx { speech, seg, ..ctx };
    let mut warnings: Vec<String> = uncertain.into_iter().collect();

    // Online first when set up: far fewer spelling mistakes, and fast.
    let mut online_result = None;
    // Sarvam times whole chunks; an installed offline model times the words.
    // (CAPTIONNEST_WORD_TIMING=0|sparse|all: developer override for measurements.)
    let word_timing = std::env::var("CAPTIONNEST_WORD_TIMING").unwrap_or_else(|_| "sparse".into());
    let timing_opts = online.as_ref().filter(|_| word_timing != "0").and_then(|_| timing_options(paths, &opts, offline_model.as_ref().ok()));
    if let Some(online) = &online {
        let code = match opts.language.as_deref() {
            Some(l) => sarvam::language_code(l),
            None => Some("unknown"),
        };
        match code {
            Some(code) => match recognize_online(&ctx, &wav, online, code, req.duration_ms).await {
                Ok(r) => online_result = Some(r),
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) if offline_model.is_ok() => {
                    log::warn!("online recognition failed, using the offline model: {e:?}");
                    warnings.push(format!("online_fallback:{}", e.message));
                }
                Err(e) => return Err(e),
            },
            None => warnings.push("online_unsupported_language".to_string()),
        }
    }
    let engine_used = if online_result.is_some() { Some(format!("sarvam:{}", sarvam::MODEL)) } else { specialised.clone() };
    let (t, missed, online_cues) = match online_result {
        Some((mut t, missed)) => {
            // Each chunk's text is pinned to its chunk. Where the voice fills only part
            // of a chunk (speech over music or chatter), an offline model listens to
            // that chunk to find where in it the words are.
            let spans: Vec<(i64, i64)> = t
                .words
                .iter()
                .filter(|p| word_timing == "all" || align::is_sparse(p, &ctx.seg.speech))
                .map(|p| (p.start_ms, p.end_ms))
                .collect();
            let timed = match timing_opts {
                Some(mut topts) if !spans.is_empty() => {
                    topts.language = t.language.clone().or(topts.language);
                    let started = std::time::Instant::now();
                    let detail = Some("timing".to_string());
                    let heard = whisper::transcribe_spans(&paths.tool("whisper-cli")?, &wav, &spans, &scratch.path().join("timing"), &topts, cancel, |p| {
                        report.stage("transcribing", Some(ONLINE_SHARE + (0.97 - ONLINE_SHARE) * p), detail.clone())
                    })
                    .await;
                    match heard {
                        Ok(mut at) => {
                            if script::uses_bengali_script(topts.language.as_deref().unwrap_or_default()) {
                                for w in &mut at.words {
                                    w.text = script::devanagari_to_bengali(&w.text);
                                }
                            }
                            log::info!("timing pass: {} of {} chunks, {} words in {:.1} s", spans.len(), t.words.len(), at.words.len(), started.elapsed().as_secs_f32());
                            Some(at.words)
                        }
                        Err(e) if e.is_cancelled() => return Err(e),
                        Err(e) => {
                            log::warn!("timing pass failed, spreading words over the speech: {e:?}");
                            None
                        }
                    }
                }
                _ => None,
            };
            let phrases = std::mem::take(&mut t.words);
            t.words = align::place_words(&phrases, timed.as_deref(), &ctx.seg.speech);
            debug_dump(&phrases, timed.as_deref(), &t.words);
            let mut seg = ctx.seg.clone();
            seg.keep_all_words = true;
            // Chunks end at pauses of 0.6 s or more: a caption never runs across one,
            // so the next sentence can't appear before it's spoken.
            seg.max_gap_ms = sarvam::JOIN_PAUSE_MS - 100;
            let mut cues = segment::segment(&t.words, &seg, Some(req.duration_ms));
            // The text is Sarvam's: if anything went missing on the way, use the plain
            // phrase captions instead of losing words.
            if !same_letters(&phrases, &cues) {
                log::warn!("timed captions lost text; using phrase timing instead");
                cues = segment::from_segments(&phrases, &ctx.seg, Some(req.duration_ms));
            }
            (t, missed, Some(cues))
        }
        None => {
            let (t, missed) = recognize(&ctx, &wav, &opts, req.duration_ms).await?;
            (t, missed, None)
        }
    };
    if req.use_gpu && !t.used_gpu && online_cues.is_none() {
        warnings.push("gpu_fallback".to_string());
    }
    if t.repaired_text {
        warnings.push("text_repaired".to_string());
    }
    let language = t.language.clone().or(spoken.clone()).unwrap_or_else(|| "und".into());
    let mut cues = online_cues.unwrap_or_else(|| segment::segment(&t.words, &ctx.seg, Some(req.duration_ms)));
    if cues.is_empty() && missed.is_empty() {
        return Err(AppError::new(
            ErrorCode::NoSpeech,
            "No speech was found in this video's sound track.",
        ));
    }
    if !missed.is_empty() {
        // Speech that couldn't be recognized becomes an empty caption the user must
        // review, so nothing is left out of the export silently.
        warnings.push(format!("unrecognized:{}", missed.len()));
        cues.extend(missed.iter().map(|&(s, e)| Cue::new(s, e, "")));
        cues.sort_by_key(|c| (c.start_ms, c.end_ms));
    }
    let original = Track {
        id: uuid::Uuid::new_v4().to_string(),
        kind: TrackKind::Original,
        language: language.clone(),
        engine: "whisper".into(),
        source_track_id: None,
        cues,
    };

    let mut tracks = vec![original.clone()];
    let target = req.caption_language.as_str();
    if target != "same" && target != language {
        let (extra, used_gpu) = translate_track(&ctx, &wav, &original, target, &req.transcribe_model_id, req.translate_model_id.as_deref(), req.use_gpu, req.duration_ms).await?;
        if req.use_gpu && !used_gpu && !warnings.contains(&"gpu_fallback".to_string()) {
            warnings.push("gpu_fallback".to_string());
        }
        tracks.extend(extra);
    }
    report.stage("finishing", None, None);
    let active_track_id = tracks.last().map(|t| t.id.clone()).unwrap_or_default();
    Ok(GenerateResult { tracks, active_track_id, detected_language: Some(language), used_gpu: t.used_gpu, warnings, model_used: engine_used })
}

/// Adds a caption language to an existing project.
pub async fn translate_existing(paths: &Paths, req: TranslateRequest, job_id: &str, cancel: &CancellationToken, report: &Reporter) -> AppResult<GenerateResult> {
    report.stage("preparing", None, None);
    let source_lang = req.source_track.language.clone();
    preflight_translation(paths, Some(&source_lang), &req.target_language, "", req.translate_model_id.as_deref())?;
    let scratch = ScratchDir::create(&paths.jobs, job_id)?;
    let mut seg = segment::Options::default();
    if let Some(n) = req.max_line_chars {
        seg.max_line_chars = n.clamp(16, 80);
    }
    let ctx = Ctx { paths, cancel, report, scratch: &scratch, seg, speech: Vec::new() };
    let plan = routes::plan(&source_lang, &req.target_language)
        .ok_or_else(|| AppError::new(ErrorCode::TranslationUnavailable, "Captions can't be translated into this language yet."))?;
    // Only Whisper-based routes need the audio again.
    let needs_audio = matches!(plan, routes::Plan::WhisperEnglish | routes::Plan::Text { from_whisper_english: true, .. });
    let wav = if needs_audio {
        let video = PathBuf::from(&req.video_path);
        if !video.is_file() {
            return Err(AppError::new(ErrorCode::FileNotFound, "The video can't be found, and it's needed for this translation. Relink it first."));
        }
        extract_wav(&ctx, &video, req.audio_stream_index, req.duration_ms).await?
    } else {
        PathBuf::new()
    };
    let (tracks, used_gpu) = translate_track(&ctx, &wav, &req.source_track, &req.target_language, "", req.translate_model_id.as_deref(), req.use_gpu, req.duration_ms).await?;
    report.stage("finishing", None, None);
    let active_track_id = tracks.last().map(|t| t.id.clone()).unwrap_or_default();
    Ok(GenerateResult { tracks, active_track_id, detected_language: Some(source_lang), used_gpu, warnings: vec![], model_used: None })
}

/// Share of the progress bar for the online requests; the rest is the offline look at
/// chunks where the voice fills only part of the chunk.
const ONLINE_SHARE: f32 = 0.8;

/// Whether the captions hold exactly the letters and digits of the online transcript
/// (after its repeated loops are removed).
fn same_letters(phrases: &[crate::asr::Word], cues: &[Cue]) -> bool {
    let key = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let want: String = phrases.iter().map(|p| key(&align::collapse_repeats(&p.text))).collect();
    let got: String = cues.iter().map(|c| key(&c.text)).collect();
    want == got
}

/// Developer aid: with CAPTIONNEST_DEBUG_DIR set, writes the online phrases, the
/// offline timed words and the placed words there as JSON.
fn debug_dump(phrases: &[crate::asr::Word], timed: Option<&[crate::asr::Word]>, placed: &[crate::asr::Word]) {
    let Ok(dir) = std::env::var("CAPTIONNEST_DEBUG_DIR") else { return };
    let rows = |ws: &[crate::asr::Word]| ws.iter().map(|w| serde_json::json!([w.start_ms, w.end_ms, w.text.trim()])).collect::<Vec<_>>();
    let doc = serde_json::json!({ "phrases": rows(phrases), "timed": rows(timed.unwrap_or(&[])), "placed": rows(placed) });
    let _ = std::fs::write(Path::new(&dir).join("online-timing.json"), serde_json::to_vec_pretty(&doc).unwrap_or_default());
}

/// The fastest installed offline model to time the words of an online transcript: the
/// smaller language model (e.g. Bangla rather than Bangla Max), else Fast, else the
/// model the user chose.
fn timing_options(paths: &Paths, opts: &whisper::Options, offline: Option<&PathBuf>) -> Option<whisper::Options> {
    let mut t = opts.clone();
    t.prompt = None;
    t.translate = false;
    t.word_timestamps = true;
    // Greedy, single pass: it only has to find where known words are.
    t.timing_only = true;
    let installed = |m: &&catalog::WhisperModel| models::whisper_installed(paths, m);
    let pick = opts
        .language
        .as_deref()
        .and_then(|l| catalog::language_models(l).filter(installed).min_by_key(|m| m.size_bytes))
        .or_else(|| catalog::whisper_model("large-v3-turbo-q5_0").filter(installed));
    t.model = match pick {
        Some(m) => models::whisper_path(paths, m),
        None => offline?.clone(),
    };
    Some(t)
}

/// Online recognition of the speech, one request per stretch between pauses. Returns
/// the transcript (one phrase per chunk) and the speech spans that came back without words.
async fn recognize_online(ctx: &Ctx<'_>, wav: &Path, online: &sarvam::Online, language: &str, duration_ms: i64) -> AppResult<(whisper::Transcript, vad::Segments)> {
    let share = ONLINE_SHARE;
    let started = std::time::Instant::now();
    let spans = if ctx.speech.is_empty() {
        whisper::fixed_windows(0, duration_ms, sarvam::MAX_CHUNK_MS)
    } else {
        // (CAPTIONNEST_JOIN_PAUSE_MS: developer override for measurements.)
        let join = std::env::var("CAPTIONNEST_JOIN_PAUSE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(sarvam::JOIN_PAUSE_MS);
        vad::plan_pause_chunks(&ctx.speech, sarvam::MAX_CHUNK_MS, join, 200, duration_ms)
    };
    let detail = Some("online".to_string());
    ctx.report.stage("transcribing", Some(0.0), detail.clone());
    let mut t = sarvam::transcribe_spans(online, wav, &spans, language, &ctx.scratch.path().join("online"), ctx.cancel, |p| {
        ctx.report.stage("transcribing", Some(p * share), detail.clone())
    })
    .await?;
    if language != "unknown" {
        t.language = t.language.or_else(|| Some(sarvam::from_code(language)));
    }
    if let Some(lang) = t.language.clone() {
        for w in &mut t.words {
            w.text = sarvam::localize(&w.text, &lang);
        }
    }
    let missed = vad::uncovered(&ctx.speech, &t.words, 300, 1_500);
    log::info!("online recognition: {} chunks in {:.1} s", spans.len(), started.elapsed().as_secs_f32());
    Ok((t, missed))
}

/// Runs the recognizer over the speech, then re-checks any stretch of speech that came
/// back without words. Returns the transcript (words filtered to speech, script repaired)
/// and the speech spans that still couldn't be recognized.
async fn recognize(ctx: &Ctx<'_>, wav: &Path, opts: &whisper::Options, duration_ms: i64) -> AppResult<(whisper::Transcript, vad::Segments)> {
    let cli = ctx.paths.tool("whisper-cli")?;
    let max_ms = vad::chunk_limit_ms(opts.language.as_deref());
    let spans = if ctx.speech.is_empty() {
        whisper::fixed_windows(0, duration_ms, max_ms)
    } else {
        vad::plan_chunks(&ctx.speech, max_ms, 1_500, 250, duration_ms)
    };
    let stage = if opts.translate { "translating" } else { "transcribing" };
    let detail = opts.translate.then(|| "whisper".to_string());
    ctx.report.stage(stage, Some(0.0), detail.clone());
    let dir = ctx.scratch.path().join(if opts.translate { "en" } else { "asr" });
    let weight = if ctx.speech.is_empty() { 1.0 } else { 0.92 };
    let mut t = whisper::transcribe_spans(&cli, wav, &spans, &dir, opts, ctx.cancel, |p| {
        ctx.report.stage(stage, Some(p * weight), detail.clone())
    })
    .await?;
    t.words = vad::keep_speech(std::mem::take(&mut t.words), &ctx.speech, 400);

    // Second chance for speech that produced no words (skipped or cut-off passages).
    let gaps = if opts.translate || opts.timing_only { Vec::new() } else { vad::uncovered(&ctx.speech, &t.words, 300, 1_500) };
    if !gaps.is_empty() {
        log::info!("re-checking {} stretches of speech without words", gaps.len());
        let retry: Vec<(i64, i64)> = gaps
            .iter()
            .flat_map(|&(s, e)| whisper::fixed_windows((s - 300).max(0), (e + 300).min(duration_ms), max_ms))
            .collect();
        // A different decoding context: no initial prompt.
        let mut retry_opts = opts.clone();
        retry_opts.prompt = None;
        if let Ok(t2) = whisper::transcribe_spans(&cli, wav, &retry, &dir, &retry_opts, ctx.cancel, |p| {
            ctx.report.stage(stage, Some(weight + p * (1.0 - weight)), detail.clone())
        })
        .await
        {
            let extra = vad::keep_speech(t2.words, &gaps, 300);
            t.used_gpu &= t2.used_gpu;
            t.repaired_text |= t2.repaired_text;
            t.words.extend(extra);
            t.words.sort_by_key(|w| w.start_ms);
        }
    }
    let lang = opts.language.clone().or_else(|| t.language.clone()).unwrap_or_default();
    if !opts.translate && script::uses_bengali_script(&lang) {
        for w in &mut t.words {
            w.text = script::devanagari_to_bengali(&w.text);
        }
    }
    let missed = if opts.translate { Vec::new() } else { vad::uncovered(&ctx.speech, &t.words, 300, 1_500) };
    Ok((t, missed))
}

/// Start times (ms) of up to three speech regions spread across the video.
fn detection_points(speech: &[(i64, i64)], first: i64) -> Vec<i64> {
    if speech.len() < 3 {
        return vec![first];
    }
    let mut pts: Vec<i64> = [0, speech.len() / 2, speech.len() * 3 / 4]
        .iter()
        .map(|&i| (speech[i].0 - 300).max(0))
        .collect();
    pts.dedup();
    pts
}

/// Combines detections: the language with the highest total probability wins.
/// Confidence is that total divided by the number of listens, so disagreement between
/// listens lowers it as much as a hesitant single listen does.
fn vote_language(votes: &[(String, f32)]) -> Option<(String, f32)> {
    let mut totals: Vec<(String, f32)> = Vec::new();
    for (lang, p) in votes {
        match totals.iter_mut().find(|(l, _)| l == lang) {
            Some(t) => t.1 += p,
            None => totals.push((lang.clone(), *p)),
        }
    }
    let (lang, total) = totals.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some((lang, total / votes.len() as f32))
}

/// Finds speech with the bundled VAD. A file with no speech at all stops here with a
/// clear message instead of running the slow recognizer.
async fn find_speech(ctx: &Ctx<'_>, wav: &Path) -> AppResult<vad::Segments> {
    let (Some(model), Ok(tool)) = (ctx.paths.vad_model(), ctx.paths.tool("whisper-vad")) else {
        return Ok(Vec::new());
    };
    let speech = vad::speech_segments(&tool, wav, &model, whisper::default_threads(), ctx.cancel).await?;
    if vad::speech_ms(&speech) < 300 {
        return Err(AppError::new(ErrorCode::NoSpeech, "No speech was found in this video's sound track."));
    }
    Ok(speech)
}

async fn extract_wav(ctx: &Ctx<'_>, video: &Path, stream: u32, duration_ms: i64) -> AppResult<PathBuf> {
    ctx.report.stage("extracting", Some(0.0), None);
    let wav = ctx.scratch.path().join("audio.wav");
    extract::extract_audio(&ctx.paths.tool("ffmpeg")?, video, stream, duration_ms, &wav, ctx.cancel, |p| {
        ctx.report.stage("extracting", Some(p), None)
    })
    .await?;
    Ok(wav)
}

/// Produces the translated track(s) for `target`. Returns new tracks (an intermediate
/// Whisper English track is included when it was needed as a pivot).
#[allow(clippy::too_many_arguments)]
async fn translate_track(
    ctx: &Ctx<'_>,
    wav: &Path,
    source: &Track,
    target: &str,
    transcribe_model_id: &str,
    translate_model_id: Option<&str>,
    use_gpu: bool,
    duration_ms: i64,
) -> AppResult<(Vec<Track>, bool)> {
    let plan = routes::plan(&source.language, target)
        .ok_or_else(|| AppError::new(ErrorCode::TranslationUnavailable, "Captions can't be translated into this language yet."))?;
    let mut out = Vec::new();
    let mut used_gpu = use_gpu;
    match plan {
        routes::Plan::Same => {}
        routes::Plan::WhisperEnglish => {
            let (t, gpu) = whisper_english(ctx, wav, source, transcribe_model_id, translate_model_id, use_gpu, duration_ms).await?;
            used_gpu = gpu;
            out.push(t);
        }
        routes::Plan::Text { from_whisper_english, packages } => {
            let base = if from_whisper_english {
                let (t, gpu) = whisper_english(ctx, wav, source, transcribe_model_id, translate_model_id, use_gpu, duration_ms).await?;
                used_gpu = gpu;
                out.push(t.clone());
                t
            } else {
                source.clone()
            };
            out.push(argos_translate(ctx, &base, target, &packages).await?);
        }
    }
    Ok((out, used_gpu))
}

async fn whisper_english(
    ctx: &Ctx<'_>,
    wav: &Path,
    source: &Track,
    transcribe_model_id: &str,
    translate_model_id: Option<&str>,
    use_gpu: bool,
    duration_ms: i64,
) -> AppResult<(Track, bool)> {
    let model = translation_model(ctx.paths, transcribe_model_id, translate_model_id)?;
    let opts = whisper::Options {
        model,
        language: (source.language != "und").then(|| source.language.clone()),
        translate: true,
        use_gpu,
        threads: whisper::default_threads(),
        word_timestamps: false,
        // No prompt: translations punctuate well without one, and a prompt can leak
        // into the output when the speech is unclear.
        prompt: None,
        timing_only: false,
    };
    // Re-use speech regions from this job, or find them (adding a language later).
    let speech = if ctx.speech.is_empty() { find_speech(ctx, wav).await.unwrap_or_default() } else { ctx.speech.clone() };
    let mut seg = ctx.seg.clone();
    seg.speech = vad::unpadded(&speech);
    let sub = Ctx { paths: ctx.paths, cancel: ctx.cancel, report: ctx.report, scratch: ctx.scratch, seg, speech };
    let (t, _) = recognize(&sub, wav, &opts, duration_ms).await?;
    let sentences = t.words.clone();
    let cues = segment::from_segments(&sentences, &sub.seg, Some(duration_ms));
    if cues.is_empty() {
        return Err(AppError::new(ErrorCode::TranslationFailed, "The English translation came out empty."));
    }
    Ok((
        Track {
            id: uuid::Uuid::new_v4().to_string(),
            kind: TrackKind::Translation,
            language: "en".into(),
            engine: "whisper-translate".into(),
            source_track_id: Some(source.id.clone()),
            cues,
        },
        t.used_gpu,
    ))
}

async fn argos_translate(ctx: &Ctx<'_>, base: &Track, target: &str, packages: &[String]) -> AppResult<Track> {
    let mut texts: Vec<String> = base.cues.iter().map(|c| c.text.clone()).collect();
    let steps = packages.len();
    for (step, id) in packages.iter().enumerate() {
        let pkg = catalog::argos_package(id).ok_or_else(|| AppError::internal(format!("unknown package {id}")))?;
        if !models::argos_installed(ctx.paths, pkg) {
            return Err(missing_language_error(pkg));
        }
        let dir = models::argos_dir(ctx.paths, pkg);
        let cancel = ctx.cancel.clone();
        let input = std::mem::take(&mut texts);
        let total = input.len().max(1);
        // Progress from the blocking thread goes through a channel.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<f32>();
        let handle = tokio::task::spawn_blocking(move || -> AppResult<Vec<String>> {
            let tr = PackageTranslator::load(&dir, whisper::default_threads())?;
            tr.translate_all(&input, &cancel, |done, _| {
                let _ = tx.send((step as f32 + done as f32 / total as f32) / steps as f32);
            })
        });
        tokio::pin!(handle);
        texts = loop {
            tokio::select! {
                Some(p) = rx.recv() => ctx.report.stage("translating", Some(p), Some("text".into())),
                res = &mut handle => break res.map_err(|e| AppError::internal(e.to_string()))??,
            }
        };
    }
    let cues = base
        .cues
        .iter()
        .zip(texts)
        .map(|(c, text)| Cue {
            check: Vec::new(),
            id: uuid::Uuid::new_v4().to_string(),
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            text: segment::wrap_text(&text, ctx.seg.max_line_chars),
        })
        .collect();
    Ok(Track {
        id: uuid::Uuid::new_v4().to_string(),
        kind: TrackKind::Translation,
        language: target.to_string(),
        engine: "argos".into(),
        source_track_id: Some(base.id.clone()),
        cues,
    })
}

fn installed_whisper(paths: &Paths, id: &str) -> AppResult<PathBuf> {
    let m = catalog::whisper_model(id).ok_or_else(|| {
        AppError::new(ErrorCode::ModelMissing, "Choose a speech recognition model in Settings.")
    })?;
    if !models::whisper_installed(paths, m) {
        return Err(AppError::new(ErrorCode::ModelMissing, "The speech recognition model isn't downloaded yet.").with_detail(m.id.clone()));
    }
    Ok(models::whisper_path(paths, m))
}

/// The model used for speech-to-English: the chosen translation model, or the
/// transcription model if it supports translation, or any installed capable model.
fn translation_model(paths: &Paths, transcribe_id: &str, translate_id: Option<&str>) -> AppResult<PathBuf> {
    let capable = |id: &str| catalog::whisper_model(id).filter(|m| m.can_translate && m.language.is_none() && models::whisper_installed(paths, m));
    let chosen = translate_id
        .and_then(capable)
        .or_else(|| capable(transcribe_id))
        .or_else(|| catalog::whisper().models.iter().find(|m| m.can_translate && m.language.is_none() && models::whisper_installed(paths, m)));
    match chosen {
        Some(m) => Ok(models::whisper_path(paths, m)),
        None => Err(AppError::new(
            ErrorCode::ModelMissing,
            "English captions from other languages need the translation model. Download it in Settings.",
        )
        .with_detail("translation-model")),
    }
}

fn missing_language_error(pkg: &catalog::ArgosPackage) -> AppError {
    AppError::new(
        ErrorCode::ModelMissing,
        format!(
            "The {} → {} language package isn't downloaded yet.",
            pkg.from_name.as_deref().unwrap_or(&pkg.from),
            pkg.to_name.as_deref().unwrap_or(&pkg.to)
        ),
    )
    .with_detail(format!("argos:{}", pkg.id))
}

fn preflight_translation(paths: &Paths, spoken: Option<&str>, target: &str, transcribe_id: &str, translate_id: Option<&str>) -> AppResult<()> {
    if target == "same" || Some(target) == spoken {
        return Ok(());
    }
    if target == "en" {
        if spoken == Some("en") {
            return Ok(());
        }
        return translation_model(paths, transcribe_id, translate_id).map(|_| ());
    }
    if !routes::argos_targets().iter().any(|t| t == crate::translate::argos_code(target)) {
        return Err(AppError::new(ErrorCode::TranslationUnavailable, "Captions can't be translated into this language yet."));
    }
    // With a known spoken language, check every package the route needs.
    if let Some(src) = spoken {
        if let Some(routes::Plan::Text { from_whisper_english, packages }) = routes::plan(src, target) {
            for id in &packages {
                if let Some(pkg) = catalog::argos_package(id) {
                    if !models::argos_installed(paths, pkg) {
                        return Err(missing_language_error(pkg));
                    }
                }
            }
            if from_whisper_english {
                translation_model(paths, transcribe_id, translate_id)?;
            }
        }
    } else if let Some(pkg) = catalog::argos_pair("en", crate::translate::argos_code(target)) {
        if !models::argos_installed(paths, pkg) {
            return Err(missing_language_error(pkg));
        }
    }
    Ok(())
}

/// Used by `probe_media` and before generation.
pub async fn probe(paths: &Paths, path: &Path) -> AppResult<probe::MediaInfo> {
    probe::probe(&paths.tool("ffprobe")?, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_listens_across_the_video() {
        let speech: Vec<(i64, i64)> = (0..10).map(|i| (i * 10_000 + 500, i * 10_000 + 4_000)).collect();
        assert_eq!(detection_points(&speech, 200), vec![200, 50_200, 70_200]);
        assert_eq!(detection_points(&speech[..2], 200), vec![200]);
    }

    #[test]
    fn majority_language_wins_and_disagreement_lowers_confidence() {
        let v = |l: &str, p: f32| (l.to_string(), p);
        let (l, c) = vote_language(&[v("en", 0.6), v("bn", 0.95), v("bn", 0.9)]).unwrap();
        assert_eq!(l, "bn");
        assert!((c - 0.6167).abs() < 0.01);
        let (l, c) = vote_language(&[v("bn", 0.97)]).unwrap();
        assert_eq!((l.as_str(), c > 0.9), ("bn", true));
        assert!(vote_language(&[]).is_none());
    }
}
