//! The two programs "Paste a video link" needs, downloaded the first time it's used and
//! kept current: yt-dlp (reads video sites) and Deno (yt-dlp runs YouTube's player code
//! in it; without a JavaScript runtime YouTube hides most formats and refuses some
//! videos, measured with yt-dlp 2026.11.23).
//!
//! Why downloaded, not bundled: video sites change every few weeks and yt-dlp follows
//! with new releases, so a copy frozen into the installer would stop working. Both come
//! from their projects' GitHub releases, found through the plain "latest" link (the
//! GitHub API rate-limits shared addresses: it refused us with HTTP 403 while testing),
//! and every file is checked against the SHA-256 the release publishes before use.
//!
//! The "onedir" builds are used, not the single-file ones: the single file unpacks itself
//! on every start, and macOS scans the unpacked files each time (12 s per run measured,
//! against 0.3 s for the onedir build after its first run).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::models::download::{self, Progress};
use crate::paths::{self, Paths};

/// Id used for the download events of these tools (shared with model downloads).
pub const TOOLS_ID: &str = "link-tools";
/// Shown before the exact size is known (yt-dlp 54 MB + Deno 39 MB in Nov 2026).
pub const APPROX_DOWNLOAD_BYTES: u64 = 95_000_000;
/// How often to look for a newer yt-dlp (sites change; yt-dlp releases follow).
const YTDLP_CHECK_MS: i64 = 3 * 24 * 3600 * 1000;
/// Deno rarely matters for yt-dlp; it's refreshed monthly.
const DENO_CHECK_MS: i64 = 30 * 24 * 3600 * 1000;

const YTDLP_REPO: &str = "yt-dlp/yt-dlp";
const DENO_REPO: &str = "denoland/deno";

fn ytdlp_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp_win.zip"
    } else {
        "yt-dlp_macos.zip"
    }
}

fn deno_asset() -> &'static str {
    if cfg!(windows) {
        "deno-x86_64-pc-windows-msvc.zip"
    } else if cfg!(target_arch = "aarch64") {
        "deno-aarch64-apple-darwin.zip"
    } else {
        "deno-x86_64-apple-darwin.zip"
    }
}

/// What's installed, written last so a half-finished install is never mistaken for one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Manifest {
    ytdlp: Option<String>,
    deno: Option<String>,
    ytdlp_checked_ms: i64,
    deno_checked_ms: i64,
}

/// Ready-to-run programs.
#[derive(Debug, Clone)]
pub struct Installed {
    pub ytdlp: PathBuf,
    pub deno: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsState {
    pub installed: bool,
    /// yt-dlp's version, e.g. "2026.11.23".
    pub version: Option<String>,
    /// About how much the first download is.
    pub download_bytes: u64,
    /// Where clips are saved unless the user picked another folder.
    pub default_folder: String,
}

fn manifest_path(p: &Paths) -> PathBuf {
    p.link_tools.join("installed.json")
}

fn read_manifest(p: &Paths) -> Manifest {
    std::fs::read(manifest_path(p)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_manifest(p: &Paths, m: &Manifest) -> AppResult<()> {
    let path = manifest_path(p);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(m).map_err(|e| AppError::internal(e.to_string()))?)?;
    paths::retry_io(|| std::fs::rename(&tmp, &path))?;
    Ok(())
}

fn ytdlp_exe(p: &Paths) -> PathBuf {
    p.link_tools.join("yt-dlp").join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp_macos" })
}

fn deno_exe(p: &Paths) -> PathBuf {
    p.link_tools.join("deno").join(if cfg!(windows) { "deno.exe" } else { "deno" })
}

pub fn installed(p: &Paths) -> Option<Installed> {
    let m = read_manifest(p);
    let (ytdlp, deno) = (ytdlp_exe(p), deno_exe(p));
    (m.ytdlp.is_some() && m.deno.is_some() && ytdlp.is_file() && deno.is_file()).then_some(Installed { ytdlp, deno })
}

pub fn state(p: &Paths, default_folder: &Path) -> ToolsState {
    let ok = installed(p).is_some();
    ToolsState {
        installed: ok,
        version: ok.then(|| read_manifest(p).ytdlp).flatten(),
        download_bytes: APPROX_DOWNLOAD_BYTES,
        default_folder: default_folder.to_string_lossy().into_owned(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The newest release's tag, read from where GitHub's "latest" page redirects.
async fn latest_tag(client: &reqwest::Client, repo: &str) -> AppResult<String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let resp = client.head(&url).send().await.map_err(network)?;
    let final_url = resp.url().as_str().to_string();
    final_url
        .rsplit_once("/releases/tag/")
        .map(|(_, tag)| tag.trim_end_matches('/').to_string())
        .filter(|t| !t.is_empty() && !t.contains('/'))
        .ok_or_else(|| tools_failed(format!("no release tag in {final_url} (HTTP {})", resp.status())))
}

/// Size of a release file (GitHub answers HEAD with the length after its redirect).
async fn asset_size(client: &reqwest::Client, url: &str) -> AppResult<u64> {
    let resp = client.head(url).send().await.map_err(network)?;
    if !resp.status().is_success() {
        return Err(tools_failed(format!("HTTP {} for {url}", resp.status())));
    }
    resp.headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| tools_failed(format!("no size for {url}")))
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> AppResult<String> {
    let resp = client.get(url).send().await.map_err(network)?;
    if !resp.status().is_success() {
        return Err(tools_failed(format!("HTTP {} for {url}", resp.status())));
    }
    resp.text().await.map_err(network)
}

/// The SHA-256 a release publishes for `asset`: yt-dlp lists all files in SHA2-256SUMS;
/// Deno has one `.sha256sum` per file (on Windows in PowerShell's Get-FileHash layout).
async fn published_sha256(client: &reqwest::Client, repo: &str, tag: &str, asset: &str) -> AppResult<String> {
    let base = format!("https://github.com/{repo}/releases/download/{tag}");
    if repo == YTDLP_REPO {
        let sums = fetch_text(client, &format!("{base}/SHA2-256SUMS")).await?;
        return sha_for(&sums, Some(asset)).ok_or_else(|| tools_failed(format!("{asset} missing from SHA2-256SUMS")));
    }
    let text = fetch_text(client, &format!("{base}/{asset}.sha256sum")).await?;
    sha_for(&text, None).ok_or_else(|| tools_failed(format!("no hash in {asset}.sha256sum")))
}

/// Finds a SHA-256 in a checksum file: on the line naming `asset`, or the first one.
fn sha_for(text: &str, asset: Option<&str>) -> Option<String> {
    let is_hex64 = |w: &str| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit());
    text.lines()
        .filter(|l| asset.is_none_or(|a| l.split_whitespace().any(|w| w.trim_start_matches('*') == a)))
        .flat_map(|l| l.split_whitespace())
        .find(|w| is_hex64(w))
        .map(|w| w.to_ascii_lowercase())
}

struct Plan {
    repo: &'static str,
    tag: String,
    asset: &'static str,
    url: String,
    size: u64,
    sha256: String,
    /// Folder under link-tools/ it unpacks into.
    folder: &'static str,
}

async fn plan(client: &reqwest::Client, repo: &'static str, asset: &'static str, folder: &'static str) -> AppResult<Plan> {
    let tag = latest_tag(client, repo).await?;
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{asset}");
    let (size, sha256) = tokio::try_join!(asset_size(client, &url), published_sha256(client, repo, &tag, asset))?;
    Ok(Plan { repo, tag, asset, url, size, sha256, folder })
}

/// Downloads and unpacks the tools that are missing or (when `refresh`) out of date.
/// Reports combined progress over every file it downloads.
pub async fn install(
    client: &reqwest::Client,
    p: &Paths,
    refresh: bool,
    cancel: &CancellationToken,
    on_progress: &(impl Fn(&'static str, Progress) + Sync),
) -> AppResult<u64> {
    std::fs::create_dir_all(&p.link_tools)?;
    let mut m = read_manifest(p);
    let now = now_ms();
    let need_ytdlp = m.ytdlp.is_none() || !ytdlp_exe(p).is_file() || (refresh && now - m.ytdlp_checked_ms > YTDLP_CHECK_MS);
    let need_deno = m.deno.is_none() || !deno_exe(p).is_file() || (refresh && now - m.deno_checked_ms > DENO_CHECK_MS);

    let mut plans = Vec::new();
    if need_ytdlp {
        plans.push(plan(client, YTDLP_REPO, ytdlp_asset(), "yt-dlp").await?);
    }
    if need_deno {
        plans.push(plan(client, DENO_REPO, deno_asset(), "deno").await?);
    }
    // Already the newest: only remember that we checked.
    plans.retain(|pl| {
        let current = if pl.repo == YTDLP_REPO { &m.ytdlp } else { &m.deno };
        let fresh = current.as_deref() == Some(pl.tag.as_str());
        let exe = if pl.repo == YTDLP_REPO { ytdlp_exe(p) } else { deno_exe(p) };
        !(fresh && exe.is_file())
    });
    if need_ytdlp {
        m.ytdlp_checked_ms = now;
    }
    if need_deno {
        m.deno_checked_ms = now;
    }

    let total: u64 = plans.iter().map(|pl| pl.size).sum();
    if total > 0 {
        paths::ensure_space(&p.link_tools, total * 3, "for the video-link downloader")?;
    }
    let mut done = 0u64;
    for pl in &plans {
        let archive = p.downloads().join(format!("{}-{}", pl.tag, pl.asset));
        let base = done;
        download::download_verified(client, &pl.url, &archive, pl.size, &pl.sha256, cancel, |pr| {
            on_progress("downloading", Progress { received: base + pr.received, total, bytes_per_sec: pr.bytes_per_sec })
        })
        .await?;
        done += pl.size;
        on_progress("installing", Progress { received: done, total, bytes_per_sec: 0.0 });
        let dest = p.link_tools.join(pl.folder);
        let (a, d) = (archive.clone(), dest.clone());
        tokio::task::spawn_blocking(move || unpack(&a, &d)).await.map_err(|e| AppError::internal(e.to_string()))??;
        let _ = std::fs::remove_file(&archive);
        if pl.repo == YTDLP_REPO {
            m.ytdlp = Some(pl.tag.clone());
        } else {
            m.deno = Some(pl.tag.clone());
        }
        log::info!("link tools: installed {} {}", pl.repo, pl.tag);
    }
    write_manifest(p, &m)?;

    // First start of a freshly unpacked program is slow on macOS (it's scanned once:
    // ~12 s for yt-dlp); do it now, while the user is already waiting for the install.
    if !plans.is_empty() {
        if let Some(t) = installed(p) {
            on_progress("installing", Progress { received: total, total, bytes_per_sec: 0.0 });
            let _ = warm_up(&t, cancel).await;
        }
    }
    Ok(total)
}

async fn warm_up(t: &Installed, cancel: &CancellationToken) -> AppResult<()> {
    for exe in [&t.ytdlp, &t.deno] {
        let mut cmd = crate::media::process::command(exe);
        cmd.arg("--version");
        tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::cancelled()),
            _ = tokio::time::timeout(Duration::from_secs(60), cmd.output()) => {}
        }
    }
    Ok(())
}

/// Unpacks a release zip into `dest`, keeping file permissions (yt-dlp's folder build has
/// ~100 executable files), via a staging folder so `dest` is replaced in one step.
fn unpack(archive: &Path, dest: &Path) -> AppResult<()> {
    let parent = dest.parent().ok_or_else(|| AppError::internal("bad destination"))?;
    let staging = tempfile::Builder::new().prefix(".unpack-").tempdir_in(parent)?;
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| damaged(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| damaged(e.to_string()))?;
        // Never write outside the staging folder, whatever the archive says.
        let Some(rel) = entry.enclosed_name() else { return Err(damaged("a file in the archive points outside its folder".into())) };
        let out = staging.path().join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755))?;
            }
        }
    }
    // yt-dlp's zip holds its files at the top; Deno's holds the one program.
    let old = parent.join(format!(".old-{}", dest.file_name().and_then(|n| n.to_str()).unwrap_or("tool")));
    let _ = std::fs::remove_dir_all(&old);
    if dest.exists() {
        paths::retry_io(|| std::fs::rename(dest, &old))?;
    }
    let kept = staging.keep();
    paths::retry_io(|| std::fs::rename(&kept, dest))?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

fn network(e: reqwest::Error) -> AppError {
    AppError::new(ErrorCode::Network, "Couldn't reach GitHub to get the video-link downloader. Check your internet connection and try again.")
        .with_detail(e.to_string())
}

fn tools_failed(detail: String) -> AppError {
    AppError::new(ErrorCode::DownloadFailed, "Couldn't get the video-link downloader right now. Please try again later.").with_detail(detail)
}

fn damaged(detail: String) -> AppError {
    AppError::new(ErrorCode::ChecksumMismatch, "The downloaded video-link downloader is damaged. Please try again.").with_detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_checksum_layouts() {
        let sums = "57654854886d0398a798895aadf5173e2749fbb5e249b9f3728022ab376e11fb  yt-dlp_macos\n\
                    43fc87fc581f64b48c366ce20b9cacb9c2949cc1ac4b9eca56fe25ac52dbb63b  yt-dlp_macos.zip\n";
        assert_eq!(sha_for(sums, Some("yt-dlp_macos.zip")).unwrap(), "43fc87fc581f64b48c366ce20b9cacb9c2949cc1ac4b9eca56fe25ac52dbb63b");
        assert_eq!(sha_for(sums, Some("yt-dlp_macos")).unwrap(), "57654854886d0398a798895aadf5173e2749fbb5e249b9f3728022ab376e11fb");
        assert!(sha_for(sums, Some("yt-dlp_win.zip")).is_none());
        // Deno's Windows file: PowerShell Get-FileHash output, upper case.
        let ps = "\r\nAlgorithm : SHA256\r\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\r\nPath      : C:\\a\\deno.zip\r\n";
        assert_eq!(sha_for(ps, None).unwrap(), "cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf");
        let mac = "9f0bf660393da2eaff044cdc7444cb08c73f235849b61d224d825a194ce7c98e  deno-aarch64-apple-darwin.zip\n";
        assert_eq!(sha_for(mac, None).unwrap(), "9f0bf660393da2eaff044cdc7444cb08c73f235849b61d224d825a194ce7c98e");
    }
}
