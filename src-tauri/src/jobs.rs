//! Background jobs: caption generation, translation, preview copies and downloads.
//!
//! Only one generation job and one preview job run at a time; starting a new one cancels
//! the previous one. Every event carries its job id, and a job that is no longer the
//! current one never delivers its result, so an outdated job can't overwrite newer work.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Generate,
    Preview,
    /// Exporting a video with captions drawn in.
    Burn,
    /// Auto-sync: finding where existing captions are spoken.
    Sync,
}

#[derive(Default)]
pub struct Jobs {
    active: Mutex<HashMap<JobKind, (String, CancellationToken)>>,
    downloads: Mutex<HashMap<String, CancellationToken>>,
}

impl Jobs {
    /// Registers a new job of `kind`, cancelling the one it replaces.
    pub fn begin(&self, kind: JobKind) -> (String, CancellationToken) {
        let id = uuid::Uuid::new_v4().to_string();
        let token = CancellationToken::new();
        if let Some((_, old)) = self.active.lock().unwrap().insert(kind, (id.clone(), token.clone())) {
            old.cancel();
        }
        (id, token)
    }

    #[cfg(test)]
    pub fn is_current(&self, kind: JobKind, id: &str) -> bool {
        self.active.lock().unwrap().get(&kind).is_some_and(|(cur, _)| cur == id)
    }

    /// Removes the job if it is still current. Returns whether it was.
    pub fn finish(&self, kind: JobKind, id: &str) -> bool {
        let mut map = self.active.lock().unwrap();
        if map.get(&kind).is_some_and(|(cur, _)| cur == id) {
            map.remove(&kind);
            true
        } else {
            false
        }
    }

    pub fn cancel(&self, id: &str) {
        let map = self.active.lock().unwrap();
        for (cur, token) in map.values() {
            if cur == id {
                token.cancel();
            }
        }
    }

    pub fn begin_download(&self, model_id: &str) -> Option<CancellationToken> {
        let mut map = self.downloads.lock().unwrap();
        if map.contains_key(model_id) {
            return None; // already downloading
        }
        let token = CancellationToken::new();
        map.insert(model_id.to_string(), token.clone());
        Some(token)
    }

    pub fn end_download(&self, model_id: &str) {
        self.downloads.lock().unwrap().remove(model_id);
    }

    pub fn cancel_download(&self, model_id: &str) {
        if let Some(t) = self.downloads.lock().unwrap().get(model_id) {
            t.cancel();
        }
    }

    /// Whether a job of `kind` is running.
    pub fn busy(&self, kind: JobKind) -> bool {
        self.active.lock().unwrap().contains_key(&kind)
    }

    pub fn active_downloads(&self) -> Vec<String> {
        self.downloads.lock().unwrap().keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent<'a> {
    pub job_id: &'a str,
    pub kind: JobKind,
    /// preparing | extracting | transcribing | translating | finishing | converting
    pub stage: &'a str,
    /// 0..1 when measurable, otherwise None (the UI shows an activity indicator).
    pub progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Outcome<T: Serialize> {
    Success { result: T },
    Failed { error: AppError },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedEvent<T: Serialize> {
    pub job_id: String,
    pub kind: JobKind,
    pub outcome: Outcome<T>,
}

/// Emits progress at most ~10 times per second per stage (plus stage changes).
pub struct Reporter {
    app: AppHandle,
    job_id: String,
    kind: JobKind,
    last: std::sync::Mutex<(String, std::time::Instant, f32)>,
}

impl Reporter {
    pub fn new(app: AppHandle, job_id: String, kind: JobKind) -> Self {
        Self {
            app,
            job_id,
            kind,
            last: std::sync::Mutex::new((String::new(), std::time::Instant::now(), -1.0)),
        }
    }

    pub fn stage(&self, stage: &str, progress: Option<f32>, detail: Option<String>) {
        {
            let mut last = self.last.lock().unwrap();
            let same_stage = last.0 == stage;
            if same_stage && progress.is_some() {
                let p = progress.unwrap();
                if last.1.elapsed() < std::time::Duration::from_millis(100) && p < 1.0 && (p - last.2).abs() < 0.05 {
                    return;
                }
            }
            *last = (stage.to_string(), std::time::Instant::now(), progress.unwrap_or(-1.0));
        }
        let _ = self.app.emit(
            "job-progress",
            ProgressEvent { job_id: &self.job_id, kind: self.kind, stage, progress, detail },
        );
    }
}

/// Runs a job's work and turns a panic into an error, so the job still finishes and the
/// interface never waits forever for a result that can't come.
pub async fn guard<T>(work: impl std::future::Future<Output = Result<T, AppError>>) -> Result<T, AppError> {
    use futures_util::FutureExt;
    match std::panic::AssertUnwindSafe(work).catch_unwind().await {
        Ok(result) => result,
        Err(panic) => {
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            log::error!("background job panicked: {what}");
            Err(AppError::internal(format!("background job stopped unexpectedly: {what}")))
        }
    }
}

/// Sends the result if the job is still current; outdated results are discarded.
pub fn finish<T: Serialize + Clone>(app: &AppHandle, jobs: &Jobs, kind: JobKind, job_id: &str, result: Result<T, AppError>) {
    if !jobs.finish(kind, job_id) {
        log::info!("discarding result of outdated job {job_id}");
        return;
    }
    let outcome = match result {
        Ok(result) => Outcome::Success { result },
        Err(e) if e.is_cancelled() => Outcome::Cancelled,
        Err(error) => {
            log::warn!("job {job_id} failed: {error:?}");
            Outcome::Failed { error }
        }
    };
    let _ = app.emit("job-finished", FinishedEvent { job_id: job_id.to_string(), kind, outcome });
}

/// A per-job scratch folder, deleted when the job ends (success, failure or cancel).
pub struct ScratchDir {
    path: std::path::PathBuf,
}

impl ScratchDir {
    pub fn create(jobs_root: &std::path::Path, id: &str) -> std::io::Result<Self> {
        let path = jobs_root.join(id);
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Removes scratch folders left behind by a crash or forced quit.
pub fn sweep_abandoned(jobs_root: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(jobs_root) {
        for e in entries.filter_map(Result::ok) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panicking_job_becomes_an_error() {
        let ok = guard(async { Ok::<_, AppError>(7) }).await;
        assert_eq!(ok.unwrap(), 7);
        let crashed: Result<i32, AppError> = guard(async { panic!("boom") }).await;
        let e = crashed.unwrap_err();
        assert_eq!(e.code, crate::error::ErrorCode::Internal);
        assert!(e.detail.unwrap_or_default().contains("boom"));
    }

    #[test]
    fn new_job_cancels_and_replaces_previous() {
        let jobs = Jobs::default();
        let (a, ta) = jobs.begin(JobKind::Generate);
        let (b, tb) = jobs.begin(JobKind::Generate);
        assert!(ta.is_cancelled());
        assert!(!tb.is_cancelled());
        assert!(!jobs.is_current(JobKind::Generate, &a));
        assert!(jobs.is_current(JobKind::Generate, &b));
        assert!(!jobs.finish(JobKind::Generate, &a), "outdated job can't finish");
        assert!(jobs.finish(JobKind::Generate, &b));
    }

    #[test]
    fn kinds_are_independent() {
        let jobs = Jobs::default();
        let (_, g) = jobs.begin(JobKind::Generate);
        let (_, p) = jobs.begin(JobKind::Preview);
        assert!(!g.is_cancelled() && !p.is_cancelled());
    }

    #[test]
    fn cancel_by_id() {
        let jobs = Jobs::default();
        let (a, ta) = jobs.begin(JobKind::Generate);
        jobs.cancel(&a);
        assert!(ta.is_cancelled());
    }

    #[test]
    fn downloads_are_deduplicated() {
        let jobs = Jobs::default();
        assert!(jobs.begin_download("small").is_some());
        assert!(jobs.begin_download("small").is_none());
        jobs.end_download("small");
        assert!(jobs.begin_download("small").is_some());
    }

    #[test]
    fn scratch_dir_is_removed_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let p = {
            let s = ScratchDir::create(root.path(), "job1").unwrap();
            std::fs::write(s.path().join("audio.wav"), b"x").unwrap();
            s.path().to_path_buf()
        };
        assert!(!p.exists());
    }
}
