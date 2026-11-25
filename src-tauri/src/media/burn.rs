//! Videos with captions drawn into the picture ("burned in").
//!
//! The app draws every caption as a transparent PNG the size of the video frame, with
//! the same renderer as the live preview (so fonts, Bengali shaping and right-to-left
//! text look exactly as on screen). FFmpeg then plays those images as one timed stream
//! (concat list: each image held for its caption's duration, a blank image in between)
//! and lays it over the video. Each video frame shows the caption whose time it falls
//! in, so timing is frame-exact. The original file is never changed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::extract::classify_ffmpeg_error;
use super::probe::MediaInfo;
use super::process;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;

/// One caption image and when it shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameCue {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Image file name inside the frames folder.
    pub file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BurnRequest {
    pub video_path: String,
    pub audio_stream_index: Option<u32>,
    pub output_path: String,
    /// Folder token from `burn_begin`, holding the images.
    pub frames_id: String,
    /// Transparent image the size of the frame, shown between captions.
    pub blank: String,
    pub cues: Vec<FrameCue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BurnResult {
    pub path: String,
    pub encoder: String,
}

/// A frame image name the app may write: letters, digits, `-` and `_`, ending in `.png`.
pub fn valid_frame_name(name: &str) -> bool {
    name.len() <= 64
        && name.ends_with(".png")
        && name[..name.len() - 4].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name.len() > 4
}

/// The input-file messages ("can't read this file") don't fit a failure to write the
/// new video: say where the problem is instead.
fn output_error(e: AppError) -> AppError {
    match e.code {
        ErrorCode::Io | ErrorCode::FileNotFound => AppError::new(
            ErrorCode::Io,
            "The new video couldn't be written to that folder. Choose another folder (for example Desktop) and export again.",
        )
        .with_detail(e.detail.unwrap_or_default()),
        _ => e,
    }
}

/// The concat list: blank until each caption, the caption for its duration, blank after.
/// Overlapping captions are trimmed so each starts after the previous one ends.
pub fn concat_list(cues: &[FrameCue], blank: &str, duration_ms: i64) -> String {
    let mut cues: Vec<&FrameCue> = cues.iter().filter(|c| c.end_ms > c.start_ms).collect();
    cues.sort_by_key(|c| (c.start_ms, c.end_ms));
    let mut out = String::from("ffconcat version 1.0\n");
    let mut push = |file: &str, ms: i64| {
        let _ = write!(out, "file '{file}'\nduration {}.{:03}\n", ms / 1000, ms % 1000);
    };
    let mut at = 0;
    for c in cues {
        let start = c.start_ms.max(at).max(0);
        let end = c.end_ms.min(duration_ms.max(c.end_ms));
        if end <= start {
            continue;
        }
        if start > at {
            push(blank, start - at);
        }
        push(&c.file, end - start);
        at = end;
    }
    push(blank, (duration_ms - at).max(40));
    // The last entry's duration only counts if the file is listed once more.
    let _ = writeln!(out, "file '{blank}'");
    out
}

/// Bitrate for the new video: a little above the original so the captions stay crisp,
/// or estimated from the picture size when the file doesn't say.
pub fn target_bitrate(width: u32, height: u32, fps: Option<f64>, source: Option<u64>) -> u64 {
    let estimate = (width as f64 * height as f64 * fps.unwrap_or(30.0).clamp(10.0, 120.0) * 0.15) as u64;
    source.map(|b| b + b / 6).unwrap_or(estimate).clamp(1_500_000, 80_000_000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoder {
    Hardware,
    /// MPEG-4 Part 2, built into FFmpeg: lower quality, but always available.
    Fallback,
}

pub async fn burn(
    ffmpeg: &Path,
    info: &MediaInfo,
    req: &BurnRequest,
    frames_dir: &Path,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(f32),
) -> AppResult<BurnResult> {
    let video = info.video.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::NoAudio, "This file has no picture, so there's nothing to draw captions on. Export an SRT instead.")
    })?;
    let out = PathBuf::from(&req.output_path);
    let rate = target_bitrate(video.width, video.height, video.frame_rate, video.bit_rate);
    let expected = rate / 8 * (info.duration_ms.max(1000) as u64 / 1000) + 50_000_000;
    paths::ensure_space(&out, expected, "to export the video")?;

    let list = frames_dir.join("captions.ffconcat");
    std::fs::write(&list, concat_list(&req.cues, &req.blank, info.duration_ms))?;

    let audio = req
        .audio_stream_index
        .and_then(|i| info.audio_tracks.iter().find(|a| a.stream_index == i))
        .or_else(|| info.default_audio.and_then(|i| info.audio_tracks.get(i)));
    let tmp = partial_path(&out);
    let mut last_err = None;
    for encoder in [Encoder::Hardware, Encoder::Fallback] {
        let _ = std::fs::remove_file(&tmp);
        let mut cmd = process::command(ffmpeg);
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-y"])
            .arg("-i")
            .arg(&info.path)
            .args(["-f", "concat", "-safe", "0", "-i"])
            .arg(&list)
            // Captions over the picture, then an even frame size and a pixel format
            // every H.264 encoder accepts.
            .args([
                "-filter_complex",
                "[1:v]format=rgba[cap];[0:v][cap]overlay=0:0:eof_action=pass:format=auto,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=nv12[v]",
            ])
            .args(["-map", "[v]"]);
        if let Some(a) = audio {
            cmd.args(["-map", &format!("0:{}", a.stream_index)]);
            if matches!(a.codec.as_str(), "aac" | "mp3" | "alac" | "ac3" | "eac3") {
                cmd.args(["-c:a", "copy"]);
            } else {
                cmd.args(["-c:a", "aac", "-b:a", "192k"]);
            }
        }
        let name = match encoder {
            Encoder::Hardware if cfg!(target_os = "macos") => {
                cmd.args(["-c:v", "h264_videotoolbox", "-allow_sw", "1", "-profile:v", "high", "-b:v", &rate.to_string()]);
                "h264_videotoolbox"
            }
            Encoder::Hardware => {
                cmd.args(["-c:v", "h264_mf", "-b:v", &rate.to_string()]);
                "h264_mf"
            }
            Encoder::Fallback => {
                cmd.args(["-c:v", "mpeg4", "-q:v", "2"]);
                "mpeg4"
            }
        };
        // Keep every frame's original timing; MP4 that starts playing before it's fully loaded.
        cmd.args(["-fps_mode", "passthrough", "-movflags", "+faststart", "-f", "mp4"]).arg(&tmp);

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
                paths::retry_io(|| std::fs::rename(&tmp, &out))?;
                on_progress(1.0);
                return Ok(BurnResult { path: out.to_string_lossy().into_owned(), encoder: name.to_string() });
            }
            Ok(o) => {
                log::warn!("burn with {name} failed: {}", o.stderr_tail);
                last_err = Some(output_error(classify_ffmpeg_error(&o.stderr_tail)));
                if last_err.as_ref().is_some_and(|e| matches!(e.code, ErrorCode::DiskFull)) {
                    break;
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last_err.unwrap_or_else(|| AppError::internal("video export failed")))
}

/// Written next to the destination, renamed into place only when complete.
fn partial_path(out: &Path) -> PathBuf {
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "video.mp4".into());
    out.with_file_name(format!(".{name}.partial"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(s: i64, e: i64, f: &str) -> FrameCue {
        FrameCue { start_ms: s, end_ms: e, file: f.into() }
    }

    #[test]
    fn concat_list_holds_each_caption_for_its_time() {
        let list = concat_list(&[cue(1_500, 3_250, "c1.png"), cue(330, 1_200, "c0.png")], "blank.png", 5_000);
        assert_eq!(
            list,
            "ffconcat version 1.0\n\
             file 'blank.png'\nduration 0.330\n\
             file 'c0.png'\nduration 0.870\n\
             file 'blank.png'\nduration 0.300\n\
             file 'c1.png'\nduration 1.750\n\
             file 'blank.png'\nduration 1.750\n\
             file 'blank.png'\n"
        );
    }

    #[test]
    fn overlapping_captions_are_trimmed_not_stacked() {
        let list = concat_list(&[cue(0, 2_000, "a.png"), cue(1_500, 3_000, "b.png")], "blank.png", 3_000);
        assert!(list.contains("file 'a.png'\nduration 2.000\nfile 'b.png'\nduration 1.000\n"), "{list}");
    }

    #[test]
    fn only_simple_png_names_are_accepted() {
        assert!(valid_frame_name("c0001.png"));
        assert!(valid_frame_name("blank.png"));
        assert!(!valid_frame_name("../x.png"));
        assert!(!valid_frame_name("a/b.png"));
        assert!(!valid_frame_name("x.exe"));
        assert!(!valid_frame_name(".png"));
    }

    #[test]
    fn bitrate_follows_the_source() {
        assert_eq!(target_bitrate(1920, 1080, Some(30.0), Some(6_000_000)), 7_000_000);
        assert_eq!(target_bitrate(1920, 1080, Some(30.0), None), 9_331_200);
        assert_eq!(target_bitrate(320, 240, Some(15.0), None), 1_500_000);
    }
}
