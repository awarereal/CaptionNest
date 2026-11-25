//! Reads a media file's streams with ffprobe.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::process;
use crate::error::{AppError, AppResult, ErrorCode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub duration_ms: i64,
    pub container: String,
    pub video: Option<VideoStream>,
    pub audio_tracks: Vec<AudioTrack>,
    /// Index into `audio_tracks` of the track to use by default.
    pub default_audio: Option<usize>,
    /// Whether the system web view is likely to play this file directly.
    pub preview_likely_playable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Option<f64>,
    /// Average and nominal frame rates differ noticeably (variable frame rate).
    pub variable_frame_rate: bool,
    /// Clockwise rotation players apply (0, 90, 180, 270), from the display matrix.
    pub rotation: u32,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    /// Absolute ffprobe stream index (`-map 0:<index>`).
    pub stream_index: u32,
    pub codec: String,
    pub channels: u32,
    pub sample_rate: u32,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Raw {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    channels: Option<u32>,
    sample_rate: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    side_data_list: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    disposition: HashMap<String, i64>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
}

pub async fn probe(ffprobe: &Path, path: &Path) -> AppResult<MediaInfo> {
    let meta = std::fs::metadata(path).map_err(|_| {
        AppError::new(ErrorCode::FileNotFound, "This video can't be found. It may have been moved or deleted.")
    })?;
    if !meta.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound, "Choose a video file, not a folder."));
    }
    let mut cmd = process::command(ffprobe);
    cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-i"]).arg(path);
    let out = process::run_streaming(cmd, &CancellationToken::new(), true, |_| {}, |_| {}).await?;
    if !out.status.success() {
        return Err(AppError::new(
            ErrorCode::CorruptMedia,
            "CaptionNest couldn't read this file. It may be damaged or not a video.",
        )
        .with_detail(out.stderr_tail));
    }
    parse(&out.stdout, path, meta.len())
}

/// Clockwise display rotation. ffprobe reports the display matrix angle
/// counter-clockwise (e.g. -90 for a phone video shot upright); old files use a tag.
fn rotation(s: &RawStream) -> u32 {
    let matrix = s.side_data_list.iter().find_map(|d| d.get("rotation").and_then(|v| v.as_f64()));
    let deg = match matrix {
        Some(r) => -r,
        None => s.tags.get("rotate").and_then(|r| r.parse::<f64>().ok()).unwrap_or(0.0),
    };
    let q = ((deg / 90.0).round() as i64).rem_euclid(4);
    (q * 90) as u32
}

pub(crate) fn parse(json: &[u8], path: &Path, size_bytes: u64) -> AppResult<MediaInfo> {
    let raw: Raw = serde_json::from_slice(json).map_err(|e| {
        AppError::new(ErrorCode::CorruptMedia, "CaptionNest couldn't read this file.").with_detail(e.to_string())
    })?;
    let format = raw.format.as_ref();
    let container = format.and_then(|f| f.format_name.clone()).unwrap_or_default();

    let mut video = None;
    let mut audio_tracks = Vec::new();
    for s in &raw.streams {
        match s.codec_type.as_deref() {
            Some("video") if video.is_none() && s.disposition.get("attached_pic").copied().unwrap_or(0) == 0 => {
                let nominal = s.r_frame_rate.as_deref().and_then(parse_rate);
                let average = s.avg_frame_rate.as_deref().and_then(parse_rate);
                let vfr = match (nominal, average) {
                    (Some(n), Some(a)) if n > 0.0 && a > 0.0 => (n - a).abs() / n > 0.01,
                    _ => false,
                };
                video = Some(VideoStream {
                    codec: s.codec_name.clone().unwrap_or_default(),
                    width: s.width.unwrap_or(0),
                    height: s.height.unwrap_or(0),
                    frame_rate: average.or(nominal),
                    variable_frame_rate: vfr,
                    rotation: rotation(s),
                    bit_rate: s.bit_rate.as_deref().and_then(|b| b.parse().ok()),
                });
            }
            Some("audio") => audio_tracks.push(AudioTrack {
                stream_index: s.index,
                codec: s.codec_name.clone().unwrap_or_default(),
                channels: s.channels.unwrap_or(0),
                sample_rate: s.sample_rate.as_deref().and_then(|r| r.parse().ok()).unwrap_or(0),
                language: s.tags.get("language").filter(|l| l.as_str() != "und").cloned(),
                title: s.tags.get("title").or_else(|| s.tags.get("handler_name")).filter(|t| !is_generic_handler(t)).cloned(),
                is_default: s.disposition.get("default").copied().unwrap_or(0) == 1,
                duration_ms: s.duration.as_deref().and_then(secs_to_ms),
            }),
            _ => {}
        }
    }

    let duration_ms = format
        .and_then(|f| f.duration.as_deref())
        .and_then(secs_to_ms)
        .or_else(|| audio_tracks.iter().filter_map(|a| a.duration_ms).max())
        .unwrap_or(0);
    let size_bytes = format.and_then(|f| f.size.as_deref()).and_then(|s| s.parse().ok()).unwrap_or(size_bytes);
    let default_audio = if audio_tracks.is_empty() {
        None
    } else {
        Some(audio_tracks.iter().position(|a| a.is_default).unwrap_or(0))
    };
    let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let preview_likely_playable = likely_playable(&container, &extension, video.as_ref(), audio_tracks.first());

    Ok(MediaInfo {
        path: path.to_string_lossy().into_owned(),
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        size_bytes,
        duration_ms,
        container,
        video,
        audio_tracks,
        default_audio,
        preview_likely_playable,
    })
}

/// Checks the file has something to transcribe. Called before generation starts.
pub fn require_audio(info: &MediaInfo) -> AppResult<()> {
    if info.audio_tracks.is_empty() {
        return Err(AppError::new(
            ErrorCode::NoAudio,
            "This video has no sound track, so there is no speech to caption.",
        ));
    }
    if info.duration_ms <= 0 {
        return Err(AppError::new(ErrorCode::CorruptMedia, "This file has no playable length."));
    }
    Ok(())
}

fn is_generic_handler(t: &str) -> bool {
    let t = t.trim().to_ascii_lowercase();
    t.is_empty() || t == "soundhandler" || t == "sound media handler" || t.starts_with("core media audio") || t == "isomedia file produced by google"
}

fn parse_rate(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then(|| n / d)
}

fn secs_to_ms(s: &str) -> Option<i64> {
    let v: f64 = s.parse().ok()?;
    (v.is_finite() && v >= 0.0).then(|| (v * 1000.0).round() as i64)
}

/// Conservative guess of what WKWebView (macOS) and WebView2 (Windows) play natively.
/// The UI still tries the original first and offers a preview copy if playback fails.
fn likely_playable(container: &str, extension: &str, video: Option<&VideoStream>, audio: Option<&AudioTrack>) -> bool {
    let c = container.to_ascii_lowercase();
    // ffprobe names both .mkv and .webm "matroska,webm"; only real WebM files play.
    let mp4_like = c.contains("mp4") || c.contains("mov");
    let webm = c.contains("webm") && extension == "webm";
    let audio_only = video.is_none() && matches!(extension, "mp3" | "m4a" | "wav" | "aac");
    let video_ok = match video {
        None => true,
        Some(v) => match v.codec.as_str() {
            "h264" => mp4_like,
            "hevc" => mp4_like && cfg!(target_os = "macos"),
            "vp8" | "vp9" => webm,
            "av1" => webm || mp4_like,
            _ => false,
        },
    };
    let audio_ok = match audio {
        None => true,
        Some(a) => match a.codec.as_str() {
            "aac" | "mp3" | "alac" | "pcm_s16le" | "flac" => mp4_like || audio_only,
            "opus" | "vorbis" => webm,
            _ => false,
        },
    };
    (mp4_like || webm || audio_only) && video_ok && audio_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    const MP4_TWO_TRACKS: &str = r#"{
      "streams": [
        {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"r_frame_rate":"30/1","avg_frame_rate":"30/1","disposition":{"default":1}},
        {"index":1,"codec_type":"audio","codec_name":"aac","channels":2,"sample_rate":"48000","duration":"12.5","tags":{"language":"ben","handler_name":"SoundHandler"},"disposition":{"default":0}},
        {"index":2,"codec_type":"audio","codec_name":"aac","channels":1,"sample_rate":"44100","tags":{"language":"eng","title":"Commentary"},"disposition":{"default":1}},
        {"index":3,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}
      ],
      "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"12.512000","size":"1048576"}
    }"#;

    #[test]
    fn parses_tracks_duration_and_default() {
        let info = parse(MP4_TWO_TRACKS.as_bytes(), Path::new("/v/My Clip.mp4"), 1).unwrap();
        assert_eq!(info.file_name, "My Clip.mp4");
        assert_eq!(info.duration_ms, 12_512);
        assert_eq!(info.size_bytes, 1_048_576);
        assert_eq!(info.audio_tracks.len(), 2);
        assert_eq!(info.audio_tracks[0].stream_index, 1);
        assert_eq!(info.audio_tracks[0].title, None, "generic handler names are hidden");
        assert_eq!(info.audio_tracks[1].title.as_deref(), Some("Commentary"));
        assert_eq!(info.default_audio, Some(1));
        let v = info.video.unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.variable_frame_rate), ("h264", 1920, false));
        assert!(info.preview_likely_playable);
    }

    #[test]
    fn detects_vfr_and_unplayable_containers() {
        let json = r#"{"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264","width":1280,"height":720,"r_frame_rate":"60/1","avg_frame_rate":"2997/100"},
            {"index":1,"codec_type":"audio","codec_name":"ac3","channels":6,"sample_rate":"48000"}],
            "format":{"format_name":"matroska,webm","duration":"5.0"}}"#;
        let info = parse(json.as_bytes(), Path::new("a.mkv"), 10).unwrap();
        assert!(info.video.as_ref().unwrap().variable_frame_rate);
        assert!(!info.preview_likely_playable);
    }

    #[test]
    fn no_audio_is_reported() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}],"format":{"format_name":"mov,mp4","duration":"3.0"}}"#;
        assert_eq!(parse(json.as_bytes(), Path::new("/x.mp4"), 1).unwrap().video.unwrap().rotation, 0);
        let info = parse(json.as_bytes(), Path::new("silent.mp4"), 1).unwrap();
        assert_eq!(info.default_audio, None);
        assert_eq!(require_audio(&info).unwrap_err().code, ErrorCode::NoAudio);
    }

    #[test]
    fn real_ffprobe_wav_fixture_parses() {
        let json = include_bytes!("../../tests/fixtures/ffprobe_wav.json");
        let info = parse(json, Path::new("bn.wav"), 1).unwrap();
        assert!(info.video.is_none());
        assert_eq!(info.audio_tracks.len(), 1);
        assert_eq!(info.audio_tracks[0].sample_rate, 16_000);
        assert!(info.duration_ms > 7_000);
    }

    #[test]
    fn garbage_is_corrupt_media() {
        assert_eq!(parse(b"nope", Path::new("x"), 1).unwrap_err().code, ErrorCode::CorruptMedia);
    }

    #[test]
    fn reads_phone_rotation_and_bitrate() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","width":1920,"height":1080,"bit_rate":"8000000",
            "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}],"format":{"format_name":"mov,mp4","duration":"3.0"}}"#;
        let v = parse(json.as_bytes(), Path::new("/x.mov"), 1).unwrap().video.unwrap();
        assert_eq!((v.rotation, v.bit_rate), (90, Some(8_000_000)));
        let old = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","tags":{"rotate":"180"}}],"format":{"duration":"1"}}"#;
        assert_eq!(parse(old.as_bytes(), Path::new("/x.mp4"), 1).unwrap().video.unwrap().rotation, 180);
    }
}
