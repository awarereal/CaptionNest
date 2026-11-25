//! `.captionnest` project files: versioned JSON, written atomically.
//!
//! A project stores a reference to the source video (never a copy), the generation
//! settings and every caption track. Saving writes a temporary file in the same folder,
//! flushes it to disk and renames it over the old file, so an interruption leaves either
//! the old or the new project, never a half-written one.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::captions::Cue;
use crate::error::{AppError, AppResult, ErrorCode};

pub const FORMAT_ID: &str = "captionnest.project";
pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub format: String,
    pub version: u32,
    pub id: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub video: VideoRef,
    pub settings: GenerationSettings,
    pub tracks: Vec<Track>,
    pub active_track_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoRef {
    /// Absolute path when the project was last saved.
    pub path: String,
    /// Path relative to the project file, used to find the video if both moved together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub duration_ms: i64,
    pub audio_stream_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationSettings {
    /// "auto" or a Whisper language code.
    pub spoken_language: String,
    #[serde(default)]
    pub detected_language: Option<String>,
    /// "same" or a language code.
    pub caption_language: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Original,
    Translation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    /// Language code of the caption text.
    pub language: String,
    /// "whisper", "whisper-translate" or "argos".
    pub engine: String,
    #[serde(default)]
    pub source_track_id: Option<String>,
    pub cues: Vec<Cue>,
}

/// Where the project's video is, after trying the saved path and the relative path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum VideoStatus {
    Found { path: String },
    /// A file exists at the path but its size differs from the saved one.
    Changed { path: String },
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedProject {
    pub project: Project,
    pub video_status: VideoStatus,
}

/// Writes the project atomically. Updates `relative_path` for the new location.
pub fn save(path: &Path, project: &Project) -> AppResult<()> {
    let mut project = project.clone();
    project.format = FORMAT_ID.into();
    project.version = CURRENT_VERSION;
    project.video.relative_path = relative_to(path, Path::new(&project.video.path));

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let json = serde_json::to_vec_pretty(&project)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".captionnest-save-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(&json)?;
    tmp.write_all(b"\n")?;
    tmp.as_file().sync_all()?;
    persist(tmp, path)?;
    Ok(())
}

/// Moves a finished temporary file into place, retrying if Windows briefly locks it.
pub fn persist(tmp: tempfile::NamedTempFile, path: &Path) -> AppResult<()> {
    let tmp_path = tmp.into_temp_path();
    crate::paths::retry_io(|| std::fs::rename(&tmp_path, path).or_else(|e| {
        // Some Windows setups refuse rename-over-existing while a reader holds the target.
        if path.exists() && cfg!(windows) { std::fs::copy(&tmp_path, path).map(|_| ()) } else { Err(e) }
    }))?;
    let _ = tmp_path.close();
    Ok(())
}

/// Reads and checks a project file, then locates its video.
pub fn load(path: &Path) -> AppResult<LoadedProject> {
    let bytes = std::fs::read(path)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(ErrorCode::ProjectInvalid, "This file isn't a readable CaptionNest project.")
            .with_detail(e.to_string())
    })?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(AppError::new(ErrorCode::ProjectInvalid, "This file isn't a CaptionNest project."));
    }
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
    if version > CURRENT_VERSION {
        return Err(AppError::new(
            ErrorCode::ProjectTooNew,
            "This project was saved by a newer version of CaptionNest. Update the app to open it.",
        ));
    }
    let value = migrate(value, version)?;
    let project: Project = serde_json::from_value(value).map_err(|e| {
        AppError::new(ErrorCode::ProjectInvalid, "This project file is damaged.").with_detail(e.to_string())
    })?;
    let video_status = locate_video(path, &project.video);
    Ok(LoadedProject { project, video_status })
}

/// Upgrades older project layouts. Version 1 is the first format, so this only
/// validates the version number today.
fn migrate(value: Value, version: u32) -> AppResult<Value> {
    match version {
        1 => Ok(value),
        _ => Err(AppError::new(ErrorCode::ProjectInvalid, "This project file has an unknown version.")),
    }
}

fn locate_video(project_path: &Path, video: &VideoRef) -> VideoStatus {
    let mut candidates = vec![PathBuf::from(&video.path)];
    if let (Some(rel), Some(dir)) = (&video.relative_path, project_path.parent()) {
        candidates.push(dir.join(rel));
    }
    let mut changed = None;
    for candidate in candidates {
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if !meta.is_file() {
                continue;
            }
            let p = candidate.to_string_lossy().into_owned();
            if meta.len() == video.size_bytes {
                return VideoStatus::Found { path: p };
            }
            changed.get_or_insert(p);
        }
    }
    match changed {
        Some(path) => VideoStatus::Changed { path },
        None => VideoStatus::Missing,
    }
}

/// Relative path from the project's folder to the video, if one can be expressed simply
/// (same folder or a subfolder, or a sibling reachable through `..`).
fn relative_to(project_path: &Path, video: &Path) -> Option<String> {
    let base = project_path.parent()?;
    let base: Vec<_> = base.components().collect();
    let target: Vec<_> = video.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None; // different drive or volume
    }
    let mut rel = PathBuf::new();
    for _ in common..base.len() {
        rel.push("..");
    }
    for c in &target[common..] {
        rel.push(c.as_os_str());
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(video_path: &str, size: u64) -> Project {
        Project {
            format: FORMAT_ID.into(),
            version: CURRENT_VERSION,
            id: "p1".into(),
            created_at_ms: 1,
            updated_at_ms: 2,
            video: VideoRef {
                path: video_path.into(),
                relative_path: None,
                file_name: "clip.mp4".into(),
                size_bytes: size,
                duration_ms: 10_000,
                audio_stream_index: Some(1),
            },
            settings: GenerationSettings {
                spoken_language: "auto".into(),
                detected_language: Some("bn".into()),
                caption_language: "en".into(),
                model_id: "large-v3-turbo-q5_0".into(),
            },
            tracks: vec![Track {
                id: "t1".into(),
                kind: TrackKind::Original,
                language: "bn".into(),
                engine: "whisper".into(),
                source_track_id: None,
                cues: vec![Cue { check: Vec::new(), id: "c1".into(), start_ms: 0, end_ms: 900, text: "আমি বাংলায় কথা বলি।\nদ্বিতীয় লাইন".into() }],
            }],
            active_track_id: Some("t1".into()),
        }
    }

    #[test]
    fn save_then_load_round_trips_unicode_and_finds_video() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("ভিডিও ফোল্ডার").join("my clip.mp4");
        std::fs::create_dir_all(video.parent().unwrap()).unwrap();
        std::fs::write(&video, b"12345").unwrap();
        let proj_path = dir.path().join("প্রকল্প.captionnest");
        let p = sample(video.to_str().unwrap(), 5);
        save(&proj_path, &p).unwrap();

        let loaded = load(&proj_path).unwrap();
        assert_eq!(loaded.project.tracks, p.tracks);
        assert_eq!(loaded.project.video.relative_path.as_deref(), Some("ভিডিও ফোল্ডার/my clip.mp4"));
        assert!(matches!(loaded.video_status, VideoStatus::Found { .. }));
    }

    #[test]
    fn finds_video_by_relative_path_after_moving_both() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("v.mp4"), b"abc").unwrap();
        let proj = old.join("p.captionnest");
        save(&proj, &sample(old.join("v.mp4").to_str().unwrap(), 3)).unwrap();

        let new = dir.path().join("new place");
        std::fs::rename(&old, &new).unwrap();
        let loaded = load(&new.join("p.captionnest")).unwrap();
        match loaded.video_status {
            VideoStatus::Found { path } => assert!(path.contains("new place")),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn reports_missing_and_changed_video() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.captionnest");
        save(&proj, &sample("/definitely/not/here.mp4", 3)).unwrap();
        assert!(matches!(load(&proj).unwrap().video_status, VideoStatus::Missing));

        let v = dir.path().join("v.mp4");
        std::fs::write(&v, b"changed size").unwrap();
        save(&proj, &sample(v.to_str().unwrap(), 3)).unwrap();
        assert!(matches!(load(&proj).unwrap().video_status, VideoStatus::Changed { .. }));
    }

    #[test]
    fn rejects_other_files_and_newer_versions() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.captionnest");
        std::fs::write(&f, b"not json").unwrap();
        assert_eq!(load(&f).unwrap_err().code, ErrorCode::ProjectInvalid);
        std::fs::write(&f, br#"{"format":"something.else","version":1}"#).unwrap();
        assert_eq!(load(&f).unwrap_err().code, ErrorCode::ProjectInvalid);
        std::fs::write(&f, br#"{"format":"captionnest.project","version":99}"#).unwrap();
        assert_eq!(load(&f).unwrap_err().code, ErrorCode::ProjectTooNew);
    }

    #[test]
    fn failed_save_leaves_previous_file_intact() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.captionnest");
        let p = sample("/v.mp4", 1);
        save(&proj, &p).unwrap();
        let before = std::fs::read(&proj).unwrap();

        // Saving into a folder that doesn't exist fails before touching the real file.
        assert!(save(&dir.path().join("missing").join("p.captionnest"), &p).is_err());
        assert_eq!(std::fs::read(&proj).unwrap(), before);
        // No temporary files are left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn save_overwrites_existing_project() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.captionnest");
        let mut p = sample("/v.mp4", 1);
        save(&proj, &p).unwrap();
        p.tracks[0].cues[0].text = "edited".into();
        save(&proj, &p).unwrap();
        assert_eq!(load(&proj).unwrap().project.tracks[0].cues[0].text, "edited");
    }
}
