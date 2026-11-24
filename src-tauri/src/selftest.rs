//! Headless self-test used by CI on installed packages:
//!
//!   CaptionNest --self-test <video> --model <ggml.bin> --out <file.srt> [--language xx]
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
        })
    }
}

pub async fn run(app: &AppHandle, t: SelfTest) -> i32 {
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
