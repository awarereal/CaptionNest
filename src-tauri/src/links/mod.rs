//! "Paste a video link": get a whole video, or only the part you need, from YouTube and
//! other video sites, then caption it. yt-dlp and Deno are downloaded on first use
//! (`tools`); reading links and fetching videos is in `fetch`.

pub mod fetch;
pub mod tools;

use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// Only one link is read at a time: reading a new one stops the previous reading.
static PROBE: Mutex<Option<CancellationToken>> = Mutex::new(None);

pub fn begin_probe() -> CancellationToken {
    let token = CancellationToken::new();
    if let Some(old) = PROBE.lock().unwrap().replace(token.clone()) {
        old.cancel();
    }
    token
}

pub fn cancel_probe() {
    if let Some(old) = PROBE.lock().unwrap().take() {
        old.cancel();
    }
}

/// Deletes saved link readings older than a day (their stream addresses have expired).
pub fn sweep_cache(dir: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let day = std::time::Duration::from_secs(24 * 3600);
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name();
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > day);
        if old && name.to_string_lossy().starts_with("info-") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
