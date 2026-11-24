//! The caption generation pipeline:
//! prepare → extract audio → transcribe → (translate) → readable cues.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::asr::{prompt, script, segment, whisper};
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
    /// Testing only: use this model file instead of a catalog model.
    #[serde(default)]
    pub model_path_override: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateResult {
    pub tracks: Vec<Track>,
    pub active_track_id: String,
    pub detected_language: Option<String>,
    pub used_gpu: bool,
    pub warnings: Vec<String>,
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
}

pub async fn generate(paths: &Paths, req: GenerateRequest, job_id: &str, cancel: &CancellationToken, report: &Reporter) -> AppResult<GenerateResult> {
    report.stage("preparing", None, None);
    let video = PathBuf::from(&req.video_path);
    if !video.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound, "The video can't be found. It may have been moved or deleted."));
    }
    let spoken = (req.spoken_language != "auto").then(|| req.spoken_language.clone());
    let transcribe_model = match &req.model_path_override {
        Some(p) => PathBuf::from(p),
        None => installed_whisper(paths, &req.transcribe_model_id)?,
    };
    // Fail fast, before minutes of work, when a needed model or package is missing.
    preflight_translation(paths, spoken.as_deref(), &req.caption_language, &req.transcribe_model_id, req.translate_model_id.as_deref())?;

    let scratch = ScratchDir::create(&paths.jobs, job_id)?;
    let mut seg = segment::Options::default();
    if let Some(n) = req.max_line_chars {
        seg.max_line_chars = n.clamp(16, 80);
    }
    let ctx = Ctx { paths, cancel, report, scratch: &scratch, seg };

    let wav = extract_wav(&ctx, &video, req.audio_stream_index, req.duration_ms).await?;

    report.stage("transcribing", None, Some("detecting".into()));
    let mut opts = whisper::Options {
        model: transcribe_model,
        language: spoken.clone(),
        translate: false,
        use_gpu: req.use_gpu,
        vad_model: paths.vad_model(),
        threads: whisper::default_threads(),
        prompt: None,
    };
    let mut uncertain = None;
    if opts.language.is_none() {
        // The Accurate model identifies languages far more reliably (on real Bangla speech
        // it was right 20 of 20 times; the Fast model 3 of 20), so it detects when present.
        let mut detect_opts = opts.clone();
        if let Some(m) = catalog::whisper_model(DETECTION_MODEL).filter(|m| models::whisper_installed(paths, m)) {
            detect_opts.model = models::whisper_path(paths, m);
        }
        if let Some((lang, p)) = whisper::detect_language(&paths.tool("whisper-cli")?, &wav, &detect_opts, cancel).await? {
            if p < 0.5 {
                uncertain = Some(format!("language_uncertain:{lang}:{p:.2}"));
            }
            opts.language = Some(lang);
        }
    }
    opts.prompt = opts.language.as_deref().and_then(prompt::initial_prompt).map(str::to_string);
    report.stage("transcribing", Some(0.0), None);
    let mut t = whisper::transcribe(&paths.tool("whisper-cli")?, &wav, &scratch.path().join("transcript"), &opts, cancel, |p| {
        report.stage("transcribing", Some(p), None)
    })
    .await?;
    let mut warnings: Vec<String> = uncertain.into_iter().collect();
    if req.use_gpu && !t.used_gpu {
        warnings.push("gpu_fallback".to_string());
    }
    if t.repaired_text {
        warnings.push("text_repaired".to_string());
    }
    let language = t.language.clone().or(spoken.clone()).unwrap_or_else(|| "und".into());
    if script::uses_bengali_script(&language) {
        for w in &mut t.words {
            w.text = script::devanagari_to_bengali(&w.text);
        }
    }
    let cues = segment::segment(&t.words, &ctx.seg, Some(req.duration_ms));
    if cues.is_empty() {
        return Err(AppError::new(
            ErrorCode::NoSpeech,
            "No speech was found in this video's sound track.",
        ));
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
    Ok(GenerateResult { tracks, active_track_id, detected_language: Some(language), used_gpu: t.used_gpu, warnings })
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
    let ctx = Ctx { paths, cancel, report, scratch: &scratch, seg };
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
    Ok(GenerateResult { tracks, active_track_id, detected_language: Some(source_lang), used_gpu, warnings: vec![] })
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
    ctx.report.stage("translating", Some(0.0), Some("whisper".into()));
    let opts = whisper::Options {
        model,
        language: (source.language != "und").then(|| source.language.clone()),
        translate: true,
        use_gpu,
        vad_model: ctx.paths.vad_model(),
        threads: whisper::default_threads(),
        prompt: prompt::initial_prompt("en").map(str::to_string),
    };
    let t = whisper::transcribe(&ctx.paths.tool("whisper-cli")?, wav, &ctx.scratch.path().join("english"), &opts, ctx.cancel, |p| {
        ctx.report.stage("translating", Some(p), Some("whisper".into()))
    })
    .await?;
    let cues = segment::segment(&t.words, &ctx.seg, Some(duration_ms));
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
    let capable = |id: &str| catalog::whisper_model(id).filter(|m| m.can_translate && models::whisper_installed(paths, m));
    let chosen = translate_id
        .and_then(capable)
        .or_else(|| capable(transcribe_id))
        .or_else(|| catalog::whisper().models.iter().find(|m| m.can_translate && models::whisper_installed(paths, m)));
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
