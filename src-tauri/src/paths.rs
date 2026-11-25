//! Where CaptionNest finds its bundled tools and keeps its data.
//!
//! Bundled tools are always resolved inside the app's resource folder, never from PATH,
//! so the app behaves the same on machines with or without developer tools.
//! Setting `CAPTIONNEST_HOME` puts all writable data under one folder (used by tests
//! and the self-test mode).

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult, ErrorCode};

#[derive(Debug, Clone)]
pub struct Paths {
    /// Bundled ffmpeg, ffprobe, whisper-cli (and DLLs on Windows).
    pub tools: PathBuf,
    /// Bundled small models (voice activity detection).
    pub bundled_models: PathBuf,
    /// Downloaded models.
    pub models: PathBuf,
    /// Settings and recent projects.
    pub config: PathBuf,
    /// Per-job scratch folders.
    pub jobs: PathBuf,
    /// Generated preview copies of videos.
    pub previews: PathBuf,
}

impl Paths {
    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        let ctx = |what: &str| {
            let what = what.to_string();
            move |e: tauri::Error| AppError::internal(format!("{what}: {e}"))
        };
        // The bundled resources; fall back to the executable's folder layouts if the
        // platform lookup fails (e.g. an unusual install location).
        let resources = match app.path().resource_dir() {
            Ok(r) if r.join("tools").is_dir() => r,
            other => {
                let exe = std::env::current_exe().map_err(|e| AppError::internal(format!("current_exe: {e}")))?;
                let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                let candidates = [dir.join("../Resources"), dir.clone()];
                match candidates.into_iter().find(|c| c.join("tools").is_dir()) {
                    Some(c) => c,
                    None => other.map_err(ctx("resource_dir"))?,
                }
            }
        };
        let (data, config, cache) = match std::env::var_os("CAPTIONNEST_HOME") {
            Some(home) => {
                let home = PathBuf::from(home);
                (home.join("data"), home.join("config"), home.join("cache"))
            }
            None => (
                app.path().app_local_data_dir().map_err(ctx("app_local_data_dir"))?,
                app.path().app_config_dir().map_err(ctx("app_config_dir"))?,
                app.path().app_cache_dir().map_err(ctx("app_cache_dir"))?,
            ),
        };
        let paths = Self {
            tools: resources.join("tools"),
            bundled_models: resources.join("bundled-models"),
            models: data.join("models"),
            config,
            jobs: cache.join("jobs"),
            previews: cache.join("previews"),
        };
        for dir in [&paths.models, &paths.config, &paths.jobs, &paths.previews] {
            std::fs::create_dir_all(dir).map_err(|e| AppError::internal(format!("create {}: {e}", dir.display())))?;
        }
        Ok(paths)
    }

    /// Path of a bundled command-line tool, with `.exe` on Windows.
    pub fn tool(&self, name: &str) -> AppResult<PathBuf> {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        let path = self.tools.join(file);
        if path.is_file() {
            Ok(path)
        } else {
            Err(AppError::new(
                ErrorCode::ToolMissing,
                "Part of CaptionNest is missing. Please reinstall the app.",
            )
            .with_detail(format!("missing bundled tool: {}", path.display())))
        }
    }

    /// Silero voice activity model bundled with the app.
    pub fn vad_model(&self) -> Option<PathBuf> {
        let p = self.bundled_models.join("ggml-silero-v6.2.0.bin");
        p.is_file().then_some(p)
    }

    pub fn whisper_models(&self) -> PathBuf {
        self.models.join("whisper")
    }

    pub fn argos_models(&self) -> PathBuf {
        self.models.join("translation")
    }

    pub fn downloads(&self) -> PathBuf {
        self.models.join(".downloads")
    }
}

/// Free bytes on the volume that holds `path` (or its nearest existing parent).
pub fn available_space(path: &Path) -> Option<u64> {
    let mut p = path;
    loop {
        if p.exists() {
            return fs4::available_space(p).ok();
        }
        p = p.parent()?;
    }
}

/// Fails with a friendly message when fewer than `needed` bytes are free at `path`.
pub fn ensure_space(path: &Path, needed: u64, what: &str) -> AppResult<()> {
    if let Some(free) = available_space(path) {
        if free < needed {
            return Err(AppError::new(
                ErrorCode::DiskFull,
                format!(
                    "Not enough free disk space {what}. About {} is needed, but only {} is free.",
                    human_bytes(needed),
                    human_bytes(free)
                ),
            ));
        }
    }
    Ok(())
}

/// Runs a file operation, retrying briefly when Windows reports the file as in use.
/// Antivirus scanners and search indexers often hold freshly written files for a moment,
/// which makes an immediate rename or delete fail with "Access is denied".
pub fn retry_io<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempt = 0;
    loop {
        match op() {
            Err(e) if attempt < 20 && is_transient_lock(&e) => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(50 * attempt.min(6)));
            }
            other => return other,
        }
    }
}

fn is_transient_lock(e: &std::io::Error) -> bool {
    // ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33)
    cfg!(windows) && (e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(5 | 32 | 33)))
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} bytes") } else if v >= 100.0 { format!("{v:.0} {}", UNITS[unit]) } else { format!("{v:.1} {}", UNITS[unit]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_is_readable() {
        assert_eq!(human_bytes(512), "512 bytes");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(574_041_195), "574 MB");
        assert_eq!(human_bytes(1_081_140_203), "1.1 GB");
    }

    #[test]
    fn available_space_walks_up_to_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(available_space(&dir.path().join("a/b/c")).is_some());
    }
}
