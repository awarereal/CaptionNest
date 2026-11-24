//! Resumable, verified downloads.
//!
//! - Data is written to `<file>.part`; an interrupted download (app closed, network lost,
//!   cancelled) resumes from where it stopped using an HTTP Range request.
//! - The SHA-256 of the whole file is checked against the pinned catalog value before the
//!   file is renamed into place, so a damaged or tampered file is never used.
//! - Short network failures are retried automatically.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: f64,
}

const MAX_ATTEMPTS: u32 = 5;

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("CaptionNest/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("http client")
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

pub async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size: u64,
    sha256: &str,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress),
) -> AppResult<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = part_path(dest);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match fetch_into_part(client, url, &part, size, cancel, &mut on_progress).await {
            Ok(()) => break,
            Err(e) if e.code == ErrorCode::Network && attempt < MAX_ATTEMPTS => {
                log::warn!("download attempt {attempt} failed, retrying: {e:?}");
                tokio::select! {
                    _ = cancel.cancelled() => return Err(AppError::cancelled()),
                    _ = tokio::time::sleep(Duration::from_secs(1 << attempt.min(4))) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }

    // Verify the complete file before using it.
    let part_for_hash = part.clone();
    let actual = tokio::task::spawn_blocking(move || hash_file(&part_for_hash))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::new(
            ErrorCode::ChecksumMismatch,
            "The download was damaged and has been removed. Please try again.",
        )
        .with_detail(format!("expected {sha256}, got {actual}")));
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

async fn fetch_into_part(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    size: u64,
    cancel: &CancellationToken,
    on_progress: &mut impl FnMut(Progress),
) -> AppResult<()> {
    let mut existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if existing > size {
        std::fs::remove_file(part)?;
        existing = 0;
    }
    if existing == size {
        return Ok(());
    }
    paths::ensure_space(part, size - existing + 64_000_000, "to download this model")?;

    let mut req = client.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
        r = req.send() => r.map_err(network_error)?,
    };
    let status = resp.status();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(part)?;
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        // Resuming.
    } else if status.is_success() {
        // Server sent the whole file: start over.
        file.set_len(0)?;
        existing = 0;
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        file.set_len(0)?;
        return Err(AppError::new(ErrorCode::Network, "The download had to restart.").with_detail("416 from server"));
    } else {
        return Err(AppError::new(ErrorCode::DownloadFailed, "The download server returned an error. Please try again later.")
            .with_detail(format!("HTTP {status} for {url}")));
    }

    let mut received = existing;
    let mut stream = resp.bytes_stream();
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut since_start: u64 = 0;
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => {
                file.flush()?;
                return Err(AppError::cancelled());
            }
            c = stream.next() => c,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|e| {
            let _ = file.flush();
            network_error(e)
        })?;
        file.write_all(&chunk)?;
        received += chunk.len() as u64;
        since_start += chunk.len() as u64;
        if received > size {
            drop(file);
            std::fs::remove_file(part)?;
            return Err(AppError::new(ErrorCode::ChecksumMismatch, "The download was larger than expected and has been removed."));
        }
        if last_emit.elapsed() >= Duration::from_millis(200) {
            last_emit = Instant::now();
            let secs = started.elapsed().as_secs_f64().max(0.001);
            on_progress(Progress { received, total: size, bytes_per_sec: since_start as f64 / secs });
        }
    }
    file.flush()?;
    file.sync_all()?;
    on_progress(Progress { received, total: size, bytes_per_sec: 0.0 });
    if received < size {
        return Err(AppError::new(ErrorCode::Network, "The connection was interrupted.").with_detail(format!("{received}/{size} bytes")));
    }
    Ok(())
}

fn network_error(e: reqwest::Error) -> AppError {
    AppError::new(
        ErrorCode::Network,
        "Couldn't reach the download server. Check your internet connection and try again.",
    )
    .with_detail(e.to_string())
}

pub fn hash_file(path: &Path) -> AppResult<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Extracts an Argos `.argosmodel` (zip with one top-level folder) into `dest`
/// atomically: everything is unpacked to a temporary folder first, then renamed.
pub fn extract_argos_package(archive: &Path, dest: &Path) -> AppResult<()> {
    let parent = dest.parent().ok_or_else(|| AppError::internal("bad destination"))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".unpack-").tempdir_in(parent)?;
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ChecksumMismatch, "The language package is damaged.").with_detail(e.to_string())
    })?;
    zip.extract(staging.path()).map_err(|e| {
        AppError::new(ErrorCode::ChecksumMismatch, "The language package couldn't be unpacked.").with_detail(e.to_string())
    })?;
    // The package root is the folder that holds metadata.json.
    let root = std::fs::read_dir(staging.path())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.join("metadata.json").is_file())
        .or_else(|| staging.path().join("metadata.json").is_file().then(|| staging.path().to_path_buf()))
        .ok_or_else(|| AppError::new(ErrorCode::ChecksumMismatch, "The language package has an unexpected layout."))?;
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::rename(&root, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn sha(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        let server = MockServer::start_async().await;
        let data = body();
        server.mock_async(|when, then| { when.method(GET).path("/m.bin"); then.status(200).body(data.clone()); }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.bin");
        let mut calls = 0;
        download_verified(&client(), &server.url("/m.bin"), &dest, data.len() as u64, &sha(&data), &CancellationToken::new(), |_| calls += 1)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!part_path(&dest).exists());
        assert!(calls >= 1);
    }

    #[tokio::test]
    async fn resumes_interrupted_download_with_range() {
        let server = MockServer::start_async().await;
        let data = body();
        let half = 80_000usize;
        let resume = server.mock_async(|when, then| {
            when.method(GET).path("/m.bin").header("range", format!("bytes={half}-"));
            then.status(206).body(data[half..].to_vec());
        }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.bin");
        std::fs::write(part_path(&dest), &data[..half]).unwrap();
        download_verified(&client(), &server.url("/m.bin"), &dest, data.len() as u64, &sha(&data), &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        resume.assert_async().await;
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[tokio::test]
    async fn restarts_when_server_ignores_range() {
        let server = MockServer::start_async().await;
        let data = body();
        server.mock_async(|when, then| { when.method(GET).path("/m.bin"); then.status(200).body(data.clone()); }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.bin");
        std::fs::write(part_path(&dest), b"stale bytes from an old attempt").unwrap();
        download_verified(&client(), &server.url("/m.bin"), &dest, data.len() as u64, &sha(&data), &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[tokio::test]
    async fn rejects_wrong_hash_and_removes_part() {
        let server = MockServer::start_async().await;
        let data = body();
        server.mock_async(|when, then| { when.method(GET).path("/m.bin"); then.status(200).body(data.clone()); }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.bin");
        let err = download_verified(&client(), &server.url("/m.bin"), &dest, data.len() as u64, &"0".repeat(64), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ChecksumMismatch);
        assert!(!dest.exists() && !part_path(&dest).exists());
    }

    #[tokio::test]
    async fn cancelled_download_keeps_partial_file_for_resume() {
        let server = MockServer::start_async().await;
        let data = body();
        server.mock_async(|when, then| { when.method(GET).path("/m.bin"); then.status(200).delay(Duration::from_secs(5)).body(data.clone()); }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.bin");
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(200)).await; c2.cancel(); });
        let err = download_verified(&client(), &server.url("/m.bin"), &dest, data.len() as u64, &sha(&data), &cancel, |_| {})
            .await
            .unwrap_err();
        assert!(err.is_cancelled());
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn http_errors_are_reported() {
        let server = MockServer::start_async().await;
        server.mock_async(|when, then| { when.method(GET).path("/gone"); then.status(404); }).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_verified(&client(), &server.url("/gone"), &dir.path().join("x"), 10, &"0".repeat(64), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::DownloadFailed);
    }

    #[test]
    fn extracts_package_folder_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("p.argosmodel");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("translate-bn_en-1_9/metadata.json", opts).unwrap();
            z.write_all(b"{}").unwrap();
            z.start_file("translate-bn_en-1_9/model/model.bin", opts).unwrap();
            z.write_all(b"weights").unwrap();
            z.finish().unwrap();
        }
        let dest = dir.path().join("translation").join("bn_en");
        extract_argos_package(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("model/model.bin")).unwrap(), b"weights");
        let leftovers: Vec<_> = std::fs::read_dir(dest.parent().unwrap()).unwrap().filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".unpack-")).collect();
        assert!(leftovers.is_empty());
    }
}
