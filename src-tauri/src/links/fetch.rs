//! Reading a video link and fetching the video, or only part of it, with yt-dlp.
//!
//! Measured choices (yt-dlp 2026.11.23, YouTube, Nov 2026):
//! - A part is fetched by ffmpeg straight from the site (yt-dlp `--download-sections`),
//!   stream-copied, so the clip keeps the original quality. From YouTube's HLS streams a
//!   2-minute 1080p60 part took 10 s; from its plain https streams 63 s (ffmpeg's single
//!   long request is throttled). So parts prefer HLS; whole videos use yt-dlp's own
//!   chunked downloader, which isn't throttled.
//! - A link is read once (`-J`, ~15 s on YouTube, most of it solving the player's
//!   JavaScript); quality choices and the download reuse that reading
//!   (`--load-info-json`), so they start at once.
//! - yt-dlp reports no progress while ffmpeg fetches a part, so progress is the size of
//!   the job's private temporary folder against an estimate from the chosen streams.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::tools::Installed;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::Reporter;
use crate::media::process;
use crate::paths::{self, Paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// MP4 with H.264 video up to 1080p and AAC sound: opens in every editor.
    Editing,
    /// The best the site has (up to 4K; may be VP9 or AV1).
    Best,
    /// Sound only: smallest, and all that captions need.
    Audio,
}

const QUALITIES: [Quality; 3] = [Quality::Editing, Quality::Best, Quality::Audio];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityChoice {
    pub quality: Quality,
    /// The site has something for this choice.
    pub available: bool,
    /// Picture height, if there's a picture.
    pub height: Option<u32>,
    /// File type of the result ("mp4", "m4a", "webm"…).
    pub ext: Option<String>,
    /// Video codec family ("avc1" is H.264, which every editor opens; "vp09", "av01"…).
    pub vcodec: Option<String>,
    /// About how big the whole video is with this choice.
    pub whole_bytes: Option<u64>,
    /// About how many bytes one second of a part takes with this choice.
    pub part_bytes_per_sec: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storyboard {
    /// Sprite sheets in time order, each `columns` × `rows` frames.
    pub sheets: Vec<String>,
    pub columns: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    /// Time between frames.
    pub frame_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    /// Identifies this reading of the link for the download.
    pub key: String,
    /// The page address (cleaned up by the site's extractor).
    pub url: String,
    pub title: String,
    pub uploader: Option<String>,
    /// "YouTube", "Facebook"…
    pub site: String,
    pub duration_ms: Option<u64>,
    /// Where the link asked to start (YouTube's `t=`), if it did.
    pub start_ms: Option<u64>,
    pub thumbnail: Option<String>,
    pub storyboard: Option<Storyboard>,
    pub choices: Vec<QualityChoice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRequest {
    pub key: String,
    pub url: String,
    pub quality: Quality,
    /// Both set: only this part. Both empty: the whole video.
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    /// Folder to save into.
    pub folder: String,
    /// Expected size, for progress (from the chosen QualityChoice).
    pub estimate_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkResult {
    pub path: String,
    pub bytes: u64,
}

/// Turns what was pasted into a web address, or says why it isn't one.
pub fn normalize_url(input: &str) -> AppResult<String> {
    let s = input.trim().trim_matches(|c| c == '<' || c == '>' || c == '"' || c == '\'');
    let bad = || AppError::new(ErrorCode::LinkFailed, "That doesn't look like a video link. Copy the address of the video page and paste it here.");
    if s.is_empty() || s.len() > 2048 || s.chars().any(char::is_whitespace) {
        return Err(bad());
    }
    let lower = s.to_ascii_lowercase();
    let url = if lower.starts_with("https://") || lower.starts_with("http://") {
        s.to_string()
    } else if !lower.contains("://") && s.contains('.') && !s.starts_with('-') && !s.starts_with('.') {
        // "youtu.be/abc" or "www.youtube.com/watch?v=…" without the https:// part.
        format!("https://{s}")
    } else {
        return Err(bad());
    };
    let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
    if host.is_empty() || !host.contains('.') || host.starts_with('-') {
        return Err(bad());
    }
    Ok(url)
}

/// yt-dlp with everything pointed at CaptionNest's own copies and folders, so the
/// user's own yt-dlp settings, PATH or cookies never change what happens.
fn ytdlp(t: &Installed, p: &Paths) -> Command {
    let mut cmd = process::command_tree(&t.ytdlp);
    cmd.args(["--no-config", "--no-update", "--color", "never", "--no-playlist", "--encoding", "utf-8"])
        .arg("--js-runtimes")
        .arg(format!("deno:{}", t.deno.display()))
        .arg("--cache-dir")
        .arg(p.link_cache.join("yt-dlp"))
        .arg("--ffmpeg-location")
        .arg(&p.tools)
        // Deno writes a code cache: keep it in CaptionNest's cache folder.
        .env("DENO_DIR", p.link_cache.join("deno"))
        // Titles in any script survive the pipe on Windows too.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    cmd
}

/// Format choice for a quality. A part prefers HLS streams (see the module notes) and
/// never uses DASH ones: ffmpeg cuts the part, and CaptionNest's ffmpeg has no DASH
/// reader (it needs libxml2). Wikimedia Commons offers DASH first, measured. Whole
/// videos can use DASH: yt-dlp downloads those itself.
fn quality_args(q: Quality, part: bool) -> Vec<String> {
    let proto = if part { "proto:m3u8," } else { "" };
    // Some sites label a DASH manifest as plain https (Commons: ext "mpd"), so both.
    let ok = if part { "[protocol!*=dash][ext!=mpd]" } else { "" };
    let video = format!("bv*{ok}+ba{ok}/b{ok}");
    let audio = format!("ba{ok}/b[vcodec=none]{ok}");
    let v: Vec<String> = match q {
        Quality::Editing => vec!["-f".into(), video, "--merge-output-format".into(), "mp4".into()],
        Quality::Best => vec!["-f".into(), video, "--merge-output-format".into(), "mp4/mkv".into()],
        Quality::Audio => vec!["-f".into(), audio],
    };
    let sort = match q {
        Quality::Editing => format!("{proto}res:1080,vcodec:h264,acodec:aac"),
        Quality::Best => format!("{proto}res,fps"),
        Quality::Audio => format!("{proto}acodec:aac"),
    };
    let mut out = v;
    out.push("-S".into());
    out.push(sort);
    out
}

fn info_path(p: &Paths, key: &str) -> PathBuf {
    p.link_cache.join(format!("info-{key}.json"))
}

/// Reads a link: title, length, preview frames and what each quality choice gives.
pub async fn probe(t: &Installed, p: &Paths, input: &str, cancel: &CancellationToken) -> AppResult<LinkInfo> {
    let url = normalize_url(input)?;
    std::fs::create_dir_all(&p.link_cache)?;
    let mut cmd = ytdlp(t, p);
    cmd.args(["-J", "--"]).arg(&url);
    let out = process::run_streaming(cmd, cancel, true, |_| {}, |_| {}).await?;
    if !out.status.success() {
        return Err(explain(&out.stderr_tail));
    }
    let info: Value = serde_json::from_slice(&out.stdout).map_err(|e| failed("the site's reply couldn't be read", e.to_string()))?;
    if info.get("_type").and_then(Value::as_str) == Some("playlist") {
        return Err(AppError::new(ErrorCode::LinkFailed, "That's a link to a playlist or channel. Open one video and copy its link instead."));
    }
    match info.get("live_status").and_then(Value::as_str) {
        Some("is_live") => return Err(AppError::new(ErrorCode::LinkFailed, "This is a live stream. It can be fetched once the stream has ended.")),
        Some("is_upcoming") => return Err(AppError::new(ErrorCode::LinkFailed, "This video hasn't started yet. Try again once it's out.")),
        _ => {}
    }

    let key = format!("{:x}{:04x}", now_ms(), std::process::id() & 0xffff);
    let file = info_path(p, &key);
    std::fs::write(&file, &out.stdout)?;

    let duration_ms = info.get("duration").and_then(Value::as_f64).filter(|d| *d > 0.0).map(|d| (d * 1000.0).round() as u64);
    let choices = choices(t, p, &file, &info, cancel).await;
    let str_of = |k: &str| info.get(k).and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty());
    Ok(LinkInfo {
        key,
        url: str_of("webpage_url").unwrap_or(url),
        title: str_of("title").unwrap_or_else(|| "Video".into()),
        uploader: str_of("channel").or_else(|| str_of("uploader")),
        site: str_of("extractor_key").map(|s| site_name(&s)).unwrap_or_default(),
        duration_ms,
        start_ms: info.get("start_time").and_then(Value::as_f64).filter(|s| *s > 0.0).map(|s| (s * 1000.0) as u64),
        thumbnail: str_of("thumbnail").filter(|u| u.starts_with("https://")),
        storyboard: storyboard(&info),
        choices,
    })
}

fn site_name(key: &str) -> String {
    match key {
        "Youtube" | "YoutubeTab" => "YouTube".into(),
        "FacebookReel" | "Facebook" => "Facebook".into(),
        "TikTok" => "TikTok".into(),
        "Instagram" => "Instagram".into(),
        "Vimeo" => "Vimeo".into(),
        "Twitter" => "X".into(),
        other => other.to_string(),
    }
}

/// Asks yt-dlp (offline, from the saved reading) what each quality would fetch.
async fn choices(t: &Installed, p: &Paths, file: &Path, info: &Value, cancel: &CancellationToken) -> Vec<QualityChoice> {
    let duration = info.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    let pick = |q: Quality, part: bool| {
        let mut cmd = ytdlp(t, p);
        cmd.arg("--load-info-json").arg(file).args(quality_args(q, part)).arg("-j");
        async move {
            let out = process::run_streaming(cmd, cancel, true, |_| {}, |_| {}).await.ok()?;
            out.status.success().then(|| serde_json::from_slice::<Value>(&out.stdout).ok()).flatten()
        }
    };
    let (e, b, a, ep, bp, ap) = tokio::join!(
        pick(Quality::Editing, false),
        pick(Quality::Best, false),
        pick(Quality::Audio, false),
        pick(Quality::Editing, true),
        pick(Quality::Best, true),
        pick(Quality::Audio, true)
    );
    let whole = [e, b, a];
    let part = [ep, bp, ap];
    QUALITIES
        .iter()
        .enumerate()
        .map(|(i, &q)| {
            let w = whole[i].as_ref();
            let pt = part[i].as_ref().or(w);
            QualityChoice {
                quality: q,
                available: w.is_some(),
                height: w.and_then(|v| v.get("height")).and_then(Value::as_u64).map(|h| h as u32),
                ext: w.and_then(|v| v.get("ext")).and_then(Value::as_str).map(str::to_string),
                vcodec: w.and_then(video_codec),
                whole_bytes: w.and_then(|v| bytes_per_sec(v, info)).map(|r| (r * duration) as u64).filter(|b| *b > 0),
                part_bytes_per_sec: pt.and_then(|v| bytes_per_sec(v, info)),
            }
        })
        .collect()
}

/// Average bytes per second of a chosen format (or merged pair). HLS streams state only
/// a peak rate (2.3× the real average on YouTube 1080p60), so their rate is borrowed from
/// the matching https stream, whose exact size is known.
fn bytes_per_sec(chosen: &Value, info: &Value) -> Option<f64> {
    let duration = info.get("duration").and_then(Value::as_f64).filter(|d| *d > 0.0)?;
    let all = info.get("formats").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    // A single chosen format is looked up in the format list: the top level of yt-dlp's
    // `-j` output carries the whole video's `filesize_approx` (743 MB for a sound-only
    // pick, measured), not the format's.
    // Its codecs, though, come from the choice: the list can leave them blank for HLS
    // streams (YouTube's sound-only HLS stream, measured).
    let formats: Vec<Value> = match chosen.get("requested_formats").and_then(Value::as_array) {
        Some(list) => list.clone(),
        None => {
            let id = chosen.get("format_id").and_then(Value::as_str);
            let entry = all.iter().find(|f| id.is_some() && f.get("format_id").and_then(Value::as_str) == id);
            let mut f = chosen.clone();
            for k in ["filesize", "filesize_approx"] {
                f[k] = entry.and_then(|e| e.get(k)).cloned().unwrap_or(Value::Null);
            }
            vec![f]
        }
    };
    let mut total = 0.0;
    for f in &formats {
        let size = f.get("filesize").or_else(|| f.get("filesize_approx")).and_then(Value::as_f64);
        let rate = size.map(|s| s / duration).or_else(|| matching_https_rate(f, all, duration)).or_else(|| {
            let tbr = f.get("tbr").and_then(Value::as_f64).unwrap_or(if is_audio(f) { 130.0 } else { 0.0 });
            Some(tbr * 1000.0 / 8.0 * if is_audio(f) { 1.0 } else { 0.45 })
        })?;
        total += rate;
    }
    (total > 0.0).then_some(total)
}

/// The chosen picture's codec family, from a merged pair or a single format.
fn video_codec(chosen: &Value) -> Option<String> {
    let codec = |f: &Value| f.get("vcodec").and_then(Value::as_str).filter(|c| *c != "none" && !c.is_empty()).map(|c| c.split('.').next().unwrap_or(c).to_string());
    match chosen.get("requested_formats").and_then(Value::as_array) {
        Some(list) => list.iter().find_map(codec),
        None => codec(chosen),
    }
}

fn is_audio(f: &Value) -> bool {
    f.get("vcodec").and_then(Value::as_str).is_none_or(|v| v == "none")
}

fn matching_https_rate(f: &Value, all: &[Value], duration: f64) -> Option<f64> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let audio = is_audio(f);
    let codec = |v: &Value| if audio { s(v, "acodec") } else { s(v, "vcodec") };
    let family = |c: &str| c.split('.').next().unwrap_or("").to_string();
    let want = family(&codec(f));
    if want.is_empty() || want == "none" {
        return None;
    }
    all.iter()
        .filter(|o| s(o, "protocol") == "https" && is_audio(o) == audio && family(&codec(o)) == want)
        .filter(|o| audio || (n(o, "height") == n(f, "height") && (n(o, "fps") - n(f, "fps")).abs() < 1.0))
        .filter_map(|o| o.get("filesize").and_then(Value::as_f64).map(|size| (o, size)))
        // Audio: the one closest to the HLS stream's stated rate (or ~128 kbps).
        .min_by(|(a, _), (b, _)| {
            let want = n(f, "tbr").max(128.0);
            (n(a, "abr") - want).abs().total_cmp(&(n(b, "abr") - want).abs())
        })
        .map(|(_, size)| size / duration)
}

/// The preview frames YouTube publishes ("storyboards"): the 160 px set, five by five
/// frames per sheet, about one frame every 5 s.
fn storyboard(info: &Value) -> Option<Storyboard> {
    let formats = info.get("formats")?.as_array()?;
    let boards: Vec<&Value> = formats.iter().filter(|f| f.get("format_note").and_then(Value::as_str) == Some("storyboard")).collect();
    let best = boards.iter().min_by_key(|f| (f.get("width").and_then(Value::as_i64).unwrap_or(0) - 160).abs())?;
    let num = |k: &str| best.get(k).and_then(Value::as_f64);
    let fps = num("fps").filter(|f| *f > 0.0)?;
    let sheets: Vec<String> = best
        .get("fragments")?
        .as_array()?
        .iter()
        .filter_map(|f| f.get("url").and_then(Value::as_str).map(str::to_string))
        .filter(|u| u.starts_with("https://"))
        .collect();
    (!sheets.is_empty()).then(|| Storyboard {
        sheets,
        columns: num("columns").unwrap_or(1.0) as u32,
        rows: num("rows").unwrap_or(1.0) as u32,
        width: num("width").unwrap_or(160.0) as u32,
        height: num("height").unwrap_or(90.0) as u32,
        frame_ms: 1000.0 / fps,
    })
}

/// "1m30s" style, for file names.
fn short_time(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    match (h, m) {
        (0, 0) => format!("{sec}s"),
        (0, _) => format!("{m}m{sec:02}s"),
        _ => format!("{h}h{m:02}m{sec:02}s"),
    }
}

/// What follows the title in the file name: the part and, for the highest quality, that.
/// yt-dlp reuses a file that already has the name, so two qualities of the same part
/// (both MP4) must not share one.
fn file_suffix(part: Option<(u64, u64)>, q: Quality) -> String {
    let mut bits = Vec::new();
    if let Some((a, b)) = part {
        bits.push(format!("{}-{}", short_time(a), short_time(b)));
    }
    if q == Quality::Best {
        bits.push("highest quality".to_string());
    }
    if bits.is_empty() {
        String::new()
    } else {
        format!(" ({})", bits.join(", "))
    }
}

/// Fetches the video (or part) into `req.folder`. Temporary files stay in `scratch`.
pub async fn download(t: &Installed, p: &Paths, req: &LinkRequest, scratch: &Path, cancel: &CancellationToken, report: &Arc<Reporter>) -> AppResult<LinkResult> {
    let url = normalize_url(&req.url)?;
    let folder = PathBuf::from(&req.folder);
    std::fs::create_dir_all(&folder).map_err(|e| {
        AppError::new(ErrorCode::Io, "CaptionNest can't save into that folder. Choose another one.").with_detail(format!("{}: {e}", folder.display()))
    })?;
    if let Some(bytes) = req.estimate_bytes {
        paths::ensure_space(&folder, bytes + bytes / 5 + 50_000_000, "to save this video")?;
    }
    let part = match (req.from_ms, req.to_ms) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    };
    let info = info_path(p, &req.key);
    let use_info = !req.key.is_empty() && req.key.chars().all(|c| c.is_ascii_hexdigit()) && info.is_file();

    let attempt = |from_info: bool| {
        let mut cmd = ytdlp(t, p);
        cmd.args(quality_args(req.quality, part.is_some()));
        if let Some((a, b)) = part {
            cmd.arg("--download-sections").arg(format!("*{:.3}-{:.3}", a as f64 / 1000.0, b as f64 / 1000.0));
        }
        let suffix = file_suffix(part, req.quality);
        cmd.arg("-P")
            .arg(format!("home:{}", folder.display()))
            .arg("-P")
            .arg(format!("temp:{}", scratch.display()))
            .arg("-o")
            .arg(format!("%(title).100B{suffix}.%(ext)s"))
            .args(["--no-mtime", "--newline", "--progress"])
            .args(["--progress-template", "postprocess:CNPOST %(progress.status)s"])
            .args(["--print", "after_move:CNFILE %(filepath)s"]);
        if from_info {
            cmd.arg("--load-info-json").arg(&info);
        } else {
            cmd.arg("--").arg(&url);
        }
        cmd
    };

    let mut tries = if use_info { vec![true, false] } else { vec![false] };
    let mut last_err = None;
    while let Some(from_info) = (!tries.is_empty()).then(|| tries.remove(0)) {
        match run_download(attempt(from_info), scratch, req.estimate_bytes, cancel, report).await.map_err(|e| {
            // A site with nothing ffmpeg can cut: say what to do instead.
            if part.is_some() && e.detail.as_deref().is_some_and(|d| d.contains("Requested format is not available")) {
                AppError::new(ErrorCode::LinkFailed, "This site doesn't let CaptionNest take just a part of this video. Choose \u{201c}The whole video\u{201d} instead.")
                    .with_detail(e.detail.clone().unwrap_or_default())
            } else {
                e
            }
        }) {
            Ok(path) => {
                let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                return Ok(LinkResult { path: path.to_string_lossy().into_owned(), bytes });
            }
            Err(e) if e.is_cancelled() => return Err(e),
            // A saved reading holds stream addresses that expire after some hours: read
            // the link again once before giving up.
            Err(e) if from_info && !tries.is_empty() => {
                log::warn!("link download from saved info failed, reading the link again: {e:?}");
                last_err = Some(e);
                let _ = std::fs::remove_dir_all(scratch).and_then(|_| std::fs::create_dir_all(scratch));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| failed("nothing was downloaded", String::new())))
}

async fn run_download(cmd: Command, scratch: &Path, estimate: Option<u64>, cancel: &CancellationToken, report: &Arc<Reporter>) -> AppResult<PathBuf> {
    report.stage("downloading", estimate.map(|_| 0.0), None);
    // Progress: the temporary folder's size against the estimate (see the module notes).
    let watch = CancellationToken::new();
    let watcher = {
        let (scratch, watch, report) = (scratch.to_path_buf(), watch.clone(), report.clone());
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let mut last = 0u64;
            loop {
                tokio::select! {
                    _ = watch.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                }
                let size = folder_size(&scratch);
                if size == last {
                    continue;
                }
                last = size;
                let speed = size as f64 / started.elapsed().as_secs_f64().max(0.5);
                let progress = estimate.filter(|e| *e > 0).map(|e| (size as f64 / e as f64).min(0.99) as f32);
                report.stage("downloading", progress, Some(format!("{size}|{}|{speed:.0}", estimate.unwrap_or(0))));
            }
        })
    };
    let mut file = None;
    let result = process::run_streaming(
        cmd,
        cancel,
        false,
        |line| {
            if let Some(path) = line.strip_prefix("CNFILE ") {
                file = Some(PathBuf::from(path.trim()));
            } else if line.starts_with("CNPOST started") {
                report.stage("finishing", None, None);
            }
        },
        |_| {},
    )
    .await;
    watch.cancel();
    let _ = watcher.await;
    let out = result?;
    if !out.status.success() {
        return Err(explain(&out.stderr_tail));
    }
    let path = file.filter(|f| f.is_file()).ok_or_else(|| failed("the finished file wasn't found", out.stderr_tail.clone()))?;
    Ok(path)
}

fn folder_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.filter_map(Result::ok)
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => folder_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn failed(what: &str, detail: String) -> AppError {
    AppError::new(ErrorCode::LinkFailed, format!("Couldn't get the video: {what}. Please try again.")).with_detail(detail)
}

/// Plain words for yt-dlp's errors (its last "ERROR:" line goes into the details).
pub fn explain(stderr: &str) -> AppError {
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:")).unwrap_or_else(|| stderr.lines().last().unwrap_or("")).to_string();
    let l = line.to_lowercase();
    let has = |s: &str| l.contains(s);
    let (code, message) = if has("unsupported url") || has("is not a valid url") {
        (ErrorCode::LinkFailed, "CaptionNest can't get a video from this link. Check that it's the address of a video page.")
    } else if has("private video") || has("this video is private") {
        (ErrorCode::LinkFailed, "This video is private. Only its owner can see it.")
    } else if has("sign in to confirm your age") || has("age-restricted") || has("inappropriate for some users") {
        (ErrorCode::LinkFailed, "This video is only shown to signed-in adults, so CaptionNest can't get it.")
    } else if has("not a bot") {
        (ErrorCode::LinkFailed, "YouTube is asking to confirm this isn't a robot, so it's blocking downloads from this internet connection for now. Try again later or from another network.")
    } else if has("members-only") || has("join this channel") || has("premium") {
        (ErrorCode::LinkFailed, "This video is only for paying members, so CaptionNest can't get it.")
    } else if has("drm") {
        (ErrorCode::LinkFailed, "This video is copy-protected, so it can't be downloaded.")
    } else if has("rate-limited") || has("http error 429") || has("too many requests") {
        (ErrorCode::LinkFailed, "The site says there have been too many downloads from this connection. Try again in an hour.")
    } else if has("not available in your country") || has("geo restrict") || has("not made this video available in your country") {
        (ErrorCode::LinkFailed, "This video isn't available in your country.")
    } else if has("video unavailable") || has("is unavailable") || has("has been removed") || has("404") || has("does not exist") {
        (ErrorCode::LinkFailed, "This video isn't available. It may have been removed, or the link may be incomplete.")
    } else if has("requested format is not available") {
        (ErrorCode::LinkFailed, "This video isn't offered in the quality you chose. Pick another quality.")
    } else if has("unable to download") || has("getaddrinfo") || has("timed out") || has("connection") || has("network is unreachable") || has("nodename nor servname") {
        (ErrorCode::Network, "Couldn't reach the video site. Check your internet connection and try again.")
    } else if has("ffmpeg exited") || has("ffmpeg returned") {
        (ErrorCode::LinkFailed, "Couldn't cut this part out of the video. Try \u{201c}The whole video\u{201d}, or another quality.")
    } else if has("no space left") {
        (ErrorCode::DiskFull, "The disk is full. Free some space, or choose another folder, and try again.")
    } else {
        (ErrorCode::LinkFailed, "Couldn't get this video. The site may have changed; CaptionNest updates its downloader every few days, so try again later.")
    };
    AppError::new(code, message).with_detail(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_cleaned_up_or_refused() {
        assert_eq!(normalize_url("  https://www.youtube.com/watch?v=aqz-KE-bpKQ&t=90 ").unwrap(), "https://www.youtube.com/watch?v=aqz-KE-bpKQ&t=90");
        assert_eq!(normalize_url("youtu.be/aqz-KE-bpKQ").unwrap(), "https://youtu.be/aqz-KE-bpKQ");
        assert_eq!(normalize_url("<https://vimeo.com/1>").unwrap(), "https://vimeo.com/1");
        for bad in ["", "hello", "ftp://x.com/a", "file:///etc/passwd", "-o evil", "https://", "two words.com", "https://localhost/x"] {
            assert!(normalize_url(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn file_name_times_are_short() {
        assert_eq!(short_time(42_000), "42s");
        assert_eq!(short_time(90_500), "1m30s");
        assert_eq!(short_time(3_725_000), "1h02m05s");
        assert_eq!(file_suffix(Some((90_000, 150_000)), Quality::Editing), " (1m30s-2m30s)");
        assert_eq!(file_suffix(Some((90_000, 150_000)), Quality::Best), " (1m30s-2m30s, highest quality)");
        assert_eq!(file_suffix(None, Quality::Best), " (highest quality)");
        assert_eq!(file_suffix(None, Quality::Audio), "");
    }

    #[test]
    fn errors_are_explained() {
        let e = explain("WARNING: x\nERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video\n");
        assert!(e.message.contains("private"));
        let e = explain("ERROR: [youtube] abc: Sign in to confirm you’re not a bot. This helps protect our community.");
        assert!(e.message.contains("robot"));
        let e = explain("ERROR: Unsupported URL: https://example.com/");
        assert!(e.message.contains("can't get a video"));
        let e = explain("ERROR: [youtube] BaW_jenozKc: This video is unavailable");
        assert!(e.message.contains("isn't available"));
        assert_eq!(explain("ERROR: Unable to download webpage: <urlopen error [Errno 8] nodename nor servname provided>").code, ErrorCode::Network);
    }

    #[test]
    fn hls_size_is_borrowed_from_the_https_stream() {
        // Trimmed from a real YouTube reading (Big Buck Bunny, CC-BY, 635 s).
        let info: Value = serde_json::json!({
            "duration": 635.0,
            "formats": [
                {"format_id": "299", "protocol": "https", "vcodec": "avc1.64002A", "acodec": "none", "height": 1080, "fps": 60, "filesize": 257_619_653u64},
                {"format_id": "140", "protocol": "https", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.5, "filesize": 10_271_496u64},
                {"format_id": "258", "protocol": "https", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 384.0, "filesize": 30_767_611u64},
            ]
        });
        let chosen = serde_json::json!({"requested_formats": [
            {"format_id": "312", "protocol": "m3u8_native", "vcodec": "avc1.64002A", "acodec": "none", "height": 1080, "fps": 60.0, "tbr": 7417.7},
            {"format_id": "234", "protocol": "m3u8_native", "vcodec": "none", "acodec": "mp4a.40.2"},
        ]});
        let rate = bytes_per_sec(&chosen, &info).unwrap();
        // A real 120 s part was 49 MB.
        let part = rate * 120.0;
        assert!((45e6..53e6).contains(&part), "{part}");

        // Sound only: one format, with the whole video's estimate at the top level.
        let mut info = info;
        // Like YouTube's list: no codec on the HLS entry, and an https stream without one.
        info["formats"].as_array_mut().unwrap().push(serde_json::json!({"format_id": "234", "protocol": "m3u8_native"}));
        info["formats"].as_array_mut().unwrap().push(serde_json::json!({"format_id": "773", "protocol": "https", "vcodec": null, "acodec": null, "filesize": 26_688_526u64}));
        let chosen = serde_json::json!({"format_id": "234", "protocol": "m3u8_native", "vcodec": "none", "acodec": "mp4a.40.2", "filesize_approx": 743_212_891u64});
        let minute = bytes_per_sec(&chosen, &info).unwrap() * 60.0;
        assert!((0.8e6..1.3e6).contains(&minute), "{minute}");
    }
}
