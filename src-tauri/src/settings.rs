//! User preferences and recent projects, stored as JSON in the app's config folder.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub use_gpu: bool,
    pub transcribe_model_id: Option<String>,
    pub translate_model_id: Option<String>,
    pub max_line_chars: usize,
    pub spoken_language: String,
    pub caption_language: String,
    pub recent_projects: Vec<RecentProject>,
    pub setup_complete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            use_gpu: true,
            transcribe_model_id: None,
            translate_model_id: None,
            max_line_chars: 42,
            spoken_language: "auto".into(),
            caption_language: "same".into(),
            recent_projects: Vec::new(),
            setup_complete: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: String,
    pub name: String,
    pub video_name: String,
    pub opened_at_ms: i64,
}

const FILE: &str = "settings.json";
const MAX_RECENT: usize = 12;

pub fn load(config_dir: &Path) -> Settings {
    std::fs::read(config_dir.join(FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, s: &Settings) -> AppResult<()> {
    std::fs::create_dir_all(config_dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(config_dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(s)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(config_dir.join(FILE)).map_err(|e| crate::error::AppError::from(e.error))?;
    Ok(())
}

impl Settings {
    /// Moves (or adds) a project to the top of the recent list.
    pub fn touch_recent(&mut self, entry: RecentProject) {
        self.recent_projects.retain(|r| r.path != entry.path);
        self.recent_projects.insert(0, entry);
        self.recent_projects.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).use_gpu);
        std::fs::write(dir.path().join(FILE), b"{broken").unwrap();
        assert_eq!(load(dir.path()).max_line_chars, 42);
    }

    #[test]
    fn save_and_load_round_trip_with_partial_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings { use_gpu: false, ..Default::default() };
        s.touch_recent(RecentProject { path: "/a.captionnest".into(), name: "a".into(), video_name: "v.mp4".into(), opened_at_ms: 1 });
        save(dir.path(), &s).unwrap();
        let back = load(dir.path());
        assert!(!back.use_gpu);
        assert_eq!(back.recent_projects.len(), 1);
        // Older files without new fields still load.
        std::fs::write(dir.path().join(FILE), br#"{"useGpu": false}"#).unwrap();
        assert_eq!(load(dir.path()).caption_language, "same");
    }

    #[test]
    fn recent_list_is_deduplicated_and_capped() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.touch_recent(RecentProject { path: format!("/p{i}"), name: String::new(), video_name: String::new(), opened_at_ms: i });
        }
        s.touch_recent(RecentProject { path: "/p5".into(), name: String::new(), video_name: String::new(), opened_at_ms: 99 });
        assert_eq!(s.recent_projects.len(), MAX_RECENT);
        assert_eq!(s.recent_projects[0].path, "/p5");
        assert_eq!(s.recent_projects.iter().filter(|r| r.path == "/p5").count(), 1);
    }
}
