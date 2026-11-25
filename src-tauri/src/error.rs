//! One error type for everything the UI can see.
//!
//! Each error carries a stable `code` (the UI picks the title, icon and recovery
//! actions from it) and a plain-language `message`. Technical details go to `detail`,
//! which the UI only shows behind a "Show details" toggle.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    FileNotFound,
    NoAudio,
    CorruptMedia,
    UnsupportedMedia,
    NoSpeech,
    DiskFull,
    ModelMissing,
    DownloadFailed,
    ChecksumMismatch,
    Network,
    Cancelled,
    ToolMissing,
    ToolFailed,
    TranslationUnavailable,
    TranslationFailed,
    ProjectInvalid,
    ProjectTooNew,
    ExportInvalid,
    Io,
    Internal,
    /// Online recognition: the API key was refused.
    OnlineKey,
    /// Online recognition: no credits left.
    OnlineCredits,
    /// Online recognition failed (service busy or an unexpected reply).
    OnlineFailed,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), detail: None }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.trim().is_empty() {
            self.detail = Some(detail);
        }
        self
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "Stopped.")
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, "Something unexpected went wrong.").with_detail(detail)
    }

    pub fn is_cancelled(&self) -> bool {
        self.code == ErrorCode::Cancelled
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        if is_disk_full(&err) {
            return AppError::new(
                ErrorCode::DiskFull,
                "Your disk is full. Free up some space and try again.",
            )
            .with_detail(err.to_string());
        }
        match err.kind() {
            std::io::ErrorKind::NotFound => {
                AppError::new(ErrorCode::FileNotFound, "A file could not be found.")
                    .with_detail(err.to_string())
            }
            std::io::ErrorKind::PermissionDenied => AppError::new(
                ErrorCode::Io,
                "CaptionNest doesn't have permission to use this file or folder.",
            )
            .with_detail(err.to_string()),
            _ => AppError::new(ErrorCode::Io, "A file could not be read or written.")
                .with_detail(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::internal(format!("JSON: {err}"))
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::internal(format!("Tauri: {err}"))
    }
}

fn is_disk_full(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::StorageFull {
        return true;
    }
    // ENOSPC (macOS/Linux = 28) and ERROR_DISK_FULL / ERROR_HANDLE_DISK_FULL on Windows.
    matches!(err.raw_os_error(), Some(28)) && cfg!(unix)
        || matches!(err.raw_os_error(), Some(112) | Some(39)) && cfg!(windows)
}

pub type AppResult<T> = Result<T, AppError>;
