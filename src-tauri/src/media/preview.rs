//! Preview copies for videos the system web view can't play (e.g. MKV, ProRes, AC-3 audio).
//!
//! The original file is never modified. The copy keeps the original timeline (same zero,
//! frames passed through without changing their timing) so captions line up exactly.
//! Strategy, cheapest first:
//! 1. Re-wrap into MP4 without re-encoding when the codecs are already playable.
//! 2. Re-encode audio only.
//! 3. Re-encode video with the system's hardware H.264 encoder (VideoToolbox / Media
//!    Foundation), scaled down to at most 1280 px wide.
//! 4. If no video encoder works, an audio-only preview so playback and timing still work.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::extract::classify_ffmpeg_error;
use super::probe::MediaInfo;
use super::process;
use crate::error::AppResult;
use crate::paths;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub path: String,
    pub has_video: bool,
}

/// Stable cache file name for a source file + chosen audio track.
pub fn cache_path(previews_dir: &Path, info: &MediaInfo, audio_stream: Option<u32>) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    info.path.hash(&mut h);
    info.size_bytes.hash(&mut h);
    audio_stream.hash(&mut h);
    if let Ok(m) = std::fs::metadata(&info.path).and_then(|m| m.modified()) {
        m.hash(&mut h);
    }
    previews_dir.join(format!("{:016x}.mp4", h.finish()))
}

pub async fn create_preview(
    ffmpeg: &Path,
    info: &MediaInfo,
    audio_stream: Option<u32>,
    out: &Path,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(f32),
) -> AppResult<Preview> {
    if out.is_file() {
        return Ok(Preview { path: out.to_string_lossy().into_owned(), has_video: info.video.is_some() });
    }
    paths::ensure_space(out, info.size_bytes / 2 + 100_000_000, "to create a preview")?;

    let video_codec = info.video.as_ref().map(|v| v.codec.as_str());
    let audio_codec = audio_stream
        .and_then(|i| info.audio_tracks.iter().find(|a| a.stream_index == i))
        .map(|a| a.codec.as_str());
    let video_copy_ok = matches!(video_codec, Some("h264")) || (cfg!(target_os = "macos") && video_codec == Some("hevc"));
    let audio_copy_ok = matches!(audio_codec, Some("aac") | Some("mp3"));

    let mut attempts: Vec<(VideoMode, bool)> = Vec::new();
    if info.video.is_some() {
        if video_copy_ok {
            attempts.push((VideoMode::Copy, audio_copy_ok));
        }
        attempts.push((VideoMode::Hardware, false));
    }
    attempts.push((VideoMode::None, false));

    let tmp = out.with_extension("partial.mp4");
    let mut last_err = None;
    for (mode, copy_audio) in attempts {
        let _ = std::fs::remove_file(&tmp);
        let cmd = build_command(ffmpeg, info, audio_stream, mode, copy_audio, &tmp);
        let total_us = info.duration_ms.max(1) as f64 * 1000.0;
        let result = process::run_streaming(
            cmd,
            cancel,
            false,
            |line| {
                if let Some(v) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = v.trim().parse::<f64>() {
                        on_progress((us / total_us).clamp(0.0, 1.0) as f32);
                    }
                }
            },
            |_| {},
        )
        .await;
        match result {
            Ok(o) if o.status.success() => {
                std::fs::rename(&tmp, out)?;
                on_progress(1.0);
                return Ok(Preview { path: out.to_string_lossy().into_owned(), has_video: mode != VideoMode::None });
            }
            Ok(o) => {
                log::warn!("preview attempt {mode:?} failed: {}", o.stderr_tail);
                last_err = Some(classify_ffmpeg_error(&o.stderr_tail));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last_err.unwrap_or_else(|| crate::error::AppError::internal("preview failed")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoMode {
    Copy,
    Hardware,
    None,
}

fn build_command(
    ffmpeg: &Path,
    info: &MediaInfo,
    audio_stream: Option<u32>,
    mode: VideoMode,
    copy_audio: bool,
    out: &Path,
) -> tokio::process::Command {
    let mut cmd = process::command(ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&info.path);
    if mode != VideoMode::None {
        cmd.args(["-map", "0:v:0"]);
    }
    if let Some(a) = audio_stream {
        cmd.args(["-map", &format!("0:{a}")]);
    }
    match mode {
        VideoMode::Copy => {
            cmd.args(["-c:v", "copy"]);
        }
        VideoMode::Hardware => {
            let encoder = if cfg!(target_os = "macos") { "h264_videotoolbox" } else { "h264_mf" };
            cmd.args(["-c:v", encoder, "-b:v", "3M", "-pix_fmt", "yuv420p"])
                .args(["-vf", "scale='min(1280,iw)':-2"])
                .args(["-fps_mode", "passthrough"]);
        }
        VideoMode::None => {
            cmd.arg("-vn");
        }
    }
    if audio_stream.is_some() {
        if copy_audio {
            cmd.args(["-c:a", "copy"]);
        } else {
            cmd.args(["-c:a", "aac", "-b:a", "160k", "-ac", "2"]);
        }
    }
    cmd.args(["-sn", "-dn", "-movflags", "+faststart", "-f", "mp4", "-progress", "pipe:1", "-nostats"]).arg(out);
    cmd
}
