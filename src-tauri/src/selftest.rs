//! Headless self-test used by CI on installed packages:
//!
//!   CaptionNest --self-test <video> --model <ggml.bin> --out <file.srt> [--language xx] [--online]
//!   CaptionNest --self-test <video> --model <ggml.bin> --out <file.srt> --language xx --resync <captions.srt> [--from s] [--to s]
//!
//! Runs the real pipeline with the bundled tools (probe → extract → transcribe →
//! segment → SRT) without showing a window, writes a JSON report next to the SRT
//! (`<out>.json`) and exits 0 on success. Windows GUI apps have no console, so the
//! report file is the way results are read.

use std::path::PathBuf;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::captions::srt;
use crate::jobs::{JobKind, Reporter};
use crate::pipeline::{self, GenerateRequest};
use crate::AppState;

#[derive(Debug, Clone)]
pub struct SelfTest {
    video: PathBuf,
    model: PathBuf,
    out: PathBuf,
    language: String,
    /// `--online`: use Sarvam with the key in SARVAM_API_KEY and/or Deepgram with the key
    /// in DEEPGRAM_API_KEY (developer testing); `--engine both|sarvam|deepgram` as in Settings.
    online: bool,
    engine: String,
    /// `--resync <captions.srt>`: auto-sync those captions to the video instead of
    /// making new ones (`--from` / `--to` in seconds limit it to a section).
    resync: Option<PathBuf>,
    section: (Option<i64>, Option<i64>),
}

impl SelfTest {
    pub fn from_args() -> Option<Self> {
        let args: Vec<String> = std::env::args().collect();
        let i = args.iter().position(|a| a == "--self-test")?;
        let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|j| args.get(j + 1)).cloned();
        let abs = |p: String| -> PathBuf {
            let p = PathBuf::from(p);
            std::fs::canonicalize(&p).unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p))
        };
        Some(Self {
            video: abs(args.get(i + 1)?.clone()),
            model: abs(get("--model")?),
            out: abs(get("--out")?),
            language: get("--language").unwrap_or_else(|| "auto".into()),
            online: args.iter().any(|a| a == "--online"),
            engine: get("--engine").unwrap_or_else(|| "both".into()),
            resync: get("--resync").map(abs),
            section: {
                let ms = |flag: &str| get(flag).and_then(|v| v.parse::<f64>().ok()).map(|s| (s * 1000.0) as i64);
                (ms("--from"), ms("--to"))
            },
        })
    }
}

/// Auto-syncs the captions of an SRT file to the video and writes the result.
async fn run_resync(app: &AppHandle, t: &SelfTest, captions: &std::path::Path) -> i32 {
    let state = app.state::<AppState>();
    let started = std::time::Instant::now();
    let result = async {
        let info = pipeline::probe(&state.paths, &t.video).await?;
        crate::media::probe::require_audio(&info)?;
        let cues = srt::parse_srt(&std::fs::read_to_string(captions)?).map_err(crate::error::AppError::internal)?;
        let req = pipeline::ResyncRequest {
            video_path: t.video.to_string_lossy().into_owned(),
            audio_stream_index: info.audio_tracks[info.default_audio.unwrap_or(0)].stream_index,
            duration_ms: info.duration_ms,
            language: t.language.clone(),
            cues: cues.clone(),
            from_ms: t.section.0,
            to_ms: t.section.1,
            transcribe_model_id: String::new(),
            use_gpu: true,
            deepgram: if t.online {
                std::env::var("DEEPGRAM_API_KEY").ok().map(|key| crate::voice::Deepgram { key, http: state.http.clone() })
            } else {
                None
            },
        };
        let reporter = Reporter::new(app.clone(), "self-test".into(), JobKind::Sync);
        let res = pipeline::resync_captions(&state.paths, req, "self-test", &tokio_util::sync::CancellationToken::new(), &reporter).await?;
        let moved: Vec<crate::captions::Cue> = cues
            .iter()
            .zip(&res.cues)
            .map(|(c, s)| crate::captions::Cue { start_ms: s.start_ms, end_ms: s.end_ms, ..c.clone() })
            .collect();
        std::fs::write(&t.out, srt::to_srt(&moved))?;
        Ok::<_, crate::error::AppError>(res)
    }
    .await;
    let (code, report) = match result {
        Ok(res) => (0, json!({ "ok": true, "seconds": started.elapsed().as_secs_f64(), "summary": res.summary, "modelUsed": res.model_used, "cues": res.cues })),
        Err(e) => (1, json!({ "ok": false, "error": e })),
    };
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    let _ = std::fs::write(t.out.with_extension("json"), &text);
    println!("{}", serde_json::to_string(&report.get("summary")).unwrap_or_default());
    code
}

pub async fn run(app: &AppHandle, t: SelfTest) -> i32 {
    if let Some(captions) = t.resync.clone() {
        return run_resync(app, &t, &captions).await;
    }
    let state = app.state::<AppState>();
    let started = std::time::Instant::now();
    let report_path = t.out.with_extension("json");
    let result = async {
        let info = pipeline::probe(&state.paths, &t.video).await?;
        crate::media::probe::require_audio(&info)?;
        let stream = info.audio_tracks[info.default_audio.unwrap_or(0)].stream_index;
        let req = GenerateRequest {
            video_path: t.video.to_string_lossy().into_owned(),
            audio_stream_index: stream,
            duration_ms: info.duration_ms,
            spoken_language: t.language.clone(),
            caption_language: "same".into(),
            transcribe_model_id: String::new(),
            translate_model_id: None,
            use_gpu: true,
            max_line_chars: None,
            use_language_models: Some(false),
            model_path_override: Some(t.model.to_string_lossy().into_owned()),
            online: if t.online {
                std::env::var("SARVAM_API_KEY").ok().map(|key| crate::asr::sarvam::Online::new(key, state.http.clone()))
            } else {
                None
            },
            deepgram: if t.online && t.engine != "sarvam" {
                std::env::var("DEEPGRAM_API_KEY").ok().map(|key| crate::voice::Deepgram { key, http: state.http.clone() })
            } else {
                None
            },
            deepgram_only: t.engine == "deepgram",
        };
        let reporter = Reporter::new(app.clone(), "self-test".into(), JobKind::Generate);
        let res = pipeline::generate(&state.paths, req, "self-test", &tokio_util::sync::CancellationToken::new(), &reporter).await?;
        let cues = &res.tracks[0].cues;
        std::fs::write(&t.out, srt::to_srt(cues))?;
        Ok::<_, crate::error::AppError>((info, res))
    }
    .await;

    let (code, report) = match result {
        Ok((info, res)) => (
            0,
            json!({
                "ok": true,
                "seconds": started.elapsed().as_secs_f64(),
                "durationMs": info.duration_ms,
                "language": res.detected_language,
                "usedGpu": res.used_gpu,
                "warnings": res.warnings,
                "modelUsed": res.model_used,
                "cues": res.tracks[0].cues.len(),
                "text": res.tracks[0].cues.iter().map(|c| c.text.replace('\n', " ")).collect::<Vec<_>>().join(" "),
                "tools": state.paths.tools.to_string_lossy(),
            }),
        ),
        Err(e) => (1, json!({ "ok": false, "error": e, "tools": state.paths.tools.to_string_lossy() })),
    };
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    println!("{text}");
    let _ = std::fs::write(&report_path, text);
    code
}
