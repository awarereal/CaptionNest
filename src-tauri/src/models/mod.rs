//! Downloadable models: the pinned catalog, what is installed, and installing/removing.

pub mod catalog;
pub mod download;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::paths::Paths;
use catalog::{ArgosPackage, WhisperModel};

/// Marker written next to a whisper model after its hash was verified.
const VERIFIED_SUFFIX: &str = ".verified";
/// Marker inside an installed translation package folder.
const INSTALLED_MARKER: &str = "captionnest-installed.json";

#[derive(Debug, Serialize, Deserialize)]
struct InstalledMarker {
    id: String,
    version: Option<String>,
    sha256: String,
}

pub fn whisper_path(paths: &Paths, m: &WhisperModel) -> PathBuf {
    paths.whisper_models().join(&m.file)
}

pub fn whisper_installed(paths: &Paths, m: &WhisperModel) -> bool {
    let p = whisper_path(paths, m);
    let ok_size = std::fs::metadata(&p).map(|md| md.len() == m.size_bytes).unwrap_or(false);
    let marker = std::fs::read_to_string(marker_path(&p)).unwrap_or_default();
    ok_size && marker.trim() == m.sha256
}

pub fn mark_whisper_verified(path: &Path, sha256: &str) -> AppResult<()> {
    std::fs::write(marker_path(path), sha256)?;
    Ok(())
}

fn marker_path(model: &Path) -> PathBuf {
    let mut s = model.as_os_str().to_owned();
    s.push(VERIFIED_SUFFIX);
    PathBuf::from(s)
}

pub fn argos_dir(paths: &Paths, p: &ArgosPackage) -> PathBuf {
    paths.argos_models().join(&p.id)
}

pub fn argos_installed(paths: &Paths, p: &ArgosPackage) -> bool {
    let dir = argos_dir(paths, p);
    let marker: Option<InstalledMarker> = std::fs::read(dir.join(INSTALLED_MARKER)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    marker.is_some_and(|m| m.sha256 == p.sha256) && dir.join("model").join("model.bin").is_file() && dir.join("sentencepiece.model").is_file()
}

pub fn installed_argos_ids(paths: &Paths) -> HashSet<String> {
    catalog::argos().packages.iter().filter(|p| argos_installed(paths, p)).map(|p| p.id.clone()).collect()
}

pub fn write_argos_marker(dir: &Path, p: &ArgosPackage) -> AppResult<()> {
    let m = InstalledMarker { id: p.id.clone(), version: p.version.clone(), sha256: p.sha256.clone() };
    std::fs::write(dir.join(INSTALLED_MARKER), serde_json::to_vec(&m)?)?;
    Ok(())
}

pub fn remove_whisper(paths: &Paths, m: &WhisperModel) -> AppResult<()> {
    let p = whisper_path(paths, m);
    for f in [marker_path(&p), p.clone(), p.with_extension("bin.part")] {
        match std::fs::remove_file(&f) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn remove_argos(paths: &Paths, p: &ArgosPackage) -> AppResult<()> {
    let dir = argos_dir(paths, p);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Total bytes used by downloaded models (including partial downloads).
pub fn disk_usage(paths: &Paths) -> u64 {
    fn walk(p: &Path) -> u64 {
        std::fs::read_dir(p)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| match e.file_type() {
                        Ok(t) if t.is_dir() => walk(&e.path()),
                        Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                        Err(_) => 0,
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    walk(&paths.models)
}
