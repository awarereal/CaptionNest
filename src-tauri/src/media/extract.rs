//! Audio extraction for speech recognition.
//!
//! Produces 16 kHz mono 16-bit WAV, the format whisper.cpp works in, while keeping the
//! original video's timeline:
//! - FFmpeg shifts all streams so the container starts at 0, the same zero video players
//!   and CapCut use.
//! - `aresample=async=1:first_pts=0` pads silence if the audio track starts later than the
//!   container, and fills gaps inside the track, so sample N is always at N / 16000 s.
//!   No chunking or silence removal happens here, so nothing can drift.

use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::process;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;

/// Bytes per second of 16 kHz mono 16-bit audio.
const WAV_BYTES_PER_SEC: u64 = 32_000;

pub async fn extract_audio(
    ffmpeg: &Path,
    input: &Path,
    stream_index: u32,
    duration_ms: i64,
    out_wav: &Path,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(f32),
) -> AppResult<()> {
    let needed = (duration_ms.max(0) as u64 / 1000 + 1) * WAV_BYTES_PER_SEC + 50_000_000;
    paths::ensure_space(out_wav, needed, "to prepare the audio")?;

    let mut cmd = process::command(ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-map", &format!("0:{stream_index}")])
        .args(["-vn", "-sn", "-dn", "-ac", "1", "-ar", "16000"])
        .args(["-af", "aresample=async=1:first_pts=0"])
        .args(["-c:a", "pcm_s16le", "-f", "wav", "-progress", "pipe:1", "-nostats"])
        .arg(out_wav);

    let total_us = (duration_ms.max(1) as f64) * 1000.0;
    let out = process::run_streaming(
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
    .await?;

    if !out.status.success() {
        return Err(classify_ffmpeg_error(&out.stderr_tail));
    }
    let size = std::fs::metadata(out_wav).map(|m| m.len()).unwrap_or(0);
    if size <= 44 {
        return Err(AppError::new(ErrorCode::NoAudio, "The selected sound track is empty."));
    }
    on_progress(1.0);
    Ok(())
}

pub fn classify_ffmpeg_error(stderr: &str) -> AppError {
    let lower = stderr.to_ascii_lowercase();
    let (code, message) = if lower.contains("no space left") || lower.contains("disk full") {
        (ErrorCode::DiskFull, "Your disk is full. Free up some space and try again.")
    } else if lower.contains("decoder") && lower.contains("not found")
        || lower.contains("unsupported codec")
        || lower.contains("no decoder")
        || lower.contains("not supported")
    {
        (ErrorCode::UnsupportedMedia, "This video uses a format CaptionNest can't decode. Try converting it to MP4 first.")
    } else if lower.contains("invalid data") || lower.contains("moov atom not found") || lower.contains("error while decoding") {
        (ErrorCode::CorruptMedia, "This video looks damaged or incomplete, so its audio couldn't be read.")
    } else if lower.contains("permission denied") {
        (ErrorCode::Io, "CaptionNest doesn't have permission to read this file.")
    } else {
        (ErrorCode::ToolFailed, "The audio couldn't be prepared from this video.")
    };
    AppError::new(code, message).with_detail(stderr.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_ffmpeg_failures() {
        assert_eq!(classify_ffmpeg_error("x.mp4: Invalid data found when processing input").code, ErrorCode::CorruptMedia);
        assert_eq!(classify_ffmpeg_error("[mov] moov atom not found").code, ErrorCode::CorruptMedia);
        assert_eq!(classify_ffmpeg_error("Decoder (codec dts) not found for input stream").code, ErrorCode::UnsupportedMedia);
        assert_eq!(classify_ffmpeg_error("Error writing: No space left on device").code, ErrorCode::DiskFull);
        assert_eq!(classify_ffmpeg_error("something odd").code, ErrorCode::ToolFailed);
    }
}
