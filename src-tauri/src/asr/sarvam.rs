//! Online speech recognition with Sarvam AI (`saaras:v4`), used instead of the offline
//! models when the user has added a Sarvam API key.
//!
//! On real Bangla speech (CaptionNest's 5-minute FLEURS test) it misspelled half as many
//! words as the best offline model (13.4% vs 26.5% WER) and took seconds instead of
//! minutes.
//!
//! Sarvam returns the text of a request with one time span for the whole request, not
//! a time per word. So timing comes from how the audio is cut: every request is one
//! stretch of speech between real pauses (at most 12 s), which pins its text to when
//! it was said. Inside a chunk, words are spread over the speech; where the voice
//! fills only part of a chunk, an offline model finds where (see `align.rs`).
//!
//! Only the audio of the speech chunks leaves the computer; the key stays in the app's
//! settings file and is never logged.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::whisper::Transcript;
use super::Word;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::wav;

const ENDPOINT: &str = "https://api.sarvam.ai/speech-to-text";
pub const MODEL: &str = "saaras:v4";
/// Longest chunk of speech per request. Sarvam returns one time span per request, so
/// chunks are short and end at pauses (see `vad::plan_pause_chunks`).
pub const MAX_CHUNK_MS: i64 = 12_000;
/// Speech separated by a pause shorter than this is sent together.
pub const JOIN_PAUSE_MS: i64 = 600;
/// Requests per minute once Sarvam has refused one (the pay-as-you-go limit is 60).
const PER_MINUTE: f64 = 54.0;
/// Requests in flight at once (the pay-as-you-go plan allows 60 a minute).
const PARALLEL: usize = 4;
const ATTEMPTS: u32 = 5;

/// Keeps a long video under Sarvam's per-minute request limit. Plans differ (60 to 4,000
/// requests a minute), so requests go out freely until Sarvam first says "too many";
/// from then on they are spaced at a steady 54 a minute.
#[derive(Debug)]
pub struct Pacer {
    limited: bool,
    tokens: f64,
    at: std::time::Instant,
}

impl Default for Pacer {
    fn default() -> Self {
        Self { limited: false, tokens: 0.0, at: std::time::Instant::now() }
    }
}

impl Pacer {
    /// Takes a slot, or says how long to wait for one.
    fn take(&mut self, now: std::time::Instant) -> Option<Duration> {
        if !self.limited {
            return None;
        }
        self.tokens = (self.tokens + now.duration_since(self.at).as_secs_f64() * PER_MINUTE / 60.0).min(4.0);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None
        } else {
            Some(Duration::from_secs_f64((1.0 - self.tokens) * 60.0 / PER_MINUTE))
        }
    }

    /// Sarvam refused a request for coming too fast: pace from now on.
    fn slow_down(&mut self, now: std::time::Instant) {
        self.limited = true;
        self.tokens = 0.0;
        self.at = now;
    }
}

/// The API key and HTTP client for one job. Debug output never shows the key.
#[derive(Clone)]
pub struct Online {
    pub key: String,
    pub http: reqwest::Client,
    pub pacer: Arc<std::sync::Mutex<Pacer>>,
}

impl Online {
    pub fn new(key: String, http: reqwest::Client) -> Self {
        Self { key, http, pacer: Arc::default() }
    }

    async fn slot(&self, cancel: &CancellationToken) -> AppResult<()> {
        loop {
            let wait = self.pacer.lock().unwrap().take(std::time::Instant::now());
            let Some(wait) = wait else { return Ok(()) };
            tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::cancelled()),
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }
}

impl std::fmt::Debug for Online {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Online { key: <hidden> }")
    }
}

/// Sarvam's code for a language, if Sarvam recognizes it.
pub fn language_code(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "bn" => "bn-IN",
        "hi" => "hi-IN",
        "en" => "en-IN",
        "ta" => "ta-IN",
        "te" => "te-IN",
        "kn" => "kn-IN",
        "ml" => "ml-IN",
        "mr" => "mr-IN",
        "gu" => "gu-IN",
        "pa" => "pa-IN",
        "or" => "od-IN",
        "as" => "as-IN",
        "ur" => "ur-IN",
        "ne" => "ne-IN",
        "sa" => "sa-IN",
        "sd" => "sd-IN",
        _ => return None,
    })
}

/// CaptionNest's code for a Sarvam language code ("bn-IN" → "bn", "od-IN" → "or").
pub fn from_code(code: &str) -> String {
    match code.split('-').next().unwrap_or(code) {
        "od" => "or".into(),
        c => c.to_ascii_lowercase(),
    }
}

/// Sarvam writes numbers the Indian way: "₹17,000" and Latin digits. Bangla captions
/// read "১৭,০০০ টাকা" (টাকা is the Bengali word for both taka and rupee).
pub fn localize(text: &str, lang: &str) -> String {
    if !matches!(lang, "bn" | "as") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '₹' {
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            let mut amount = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || ((d == ',' || d == '.') && !amount.is_empty()) {
                    amount.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            // A trailing comma or period belongs to the sentence, not the number.
            let tail: String = amount.chars().rev().take_while(|c| *c == ',' || *c == '.').collect();
            let amount = &amount[..amount.len() - tail.len()];
            if amount.is_empty() {
                out.push('₹');
            } else {
                out.push_str(&bengali_digits(amount));
                // Assamese writes the currency word টকা.
                out.push_str(if lang == "as" { " টকা" } else { " টাকা" });
            }
            out.push_str(&tail);
        } else {
            out.push(c);
        }
    }
    bengali_digits(&out)
}

fn bengali_digits(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_digit() { char::from_u32(0x09E6 + (c as u32 - '0' as u32)).unwrap_or(c) } else { c }).collect()
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    transcript: String,
    language_code: Option<String>,
    timestamps: Option<Timestamps>,
}

#[derive(Deserialize)]
struct Timestamps {
    #[serde(default)]
    words: Vec<String>,
    #[serde(default)]
    start_time_seconds: Vec<f64>,
    #[serde(default)]
    end_time_seconds: Vec<f64>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorInner,
}

#[derive(Deserialize)]
struct ErrorInner {
    message: Option<String>,
    code: Option<String>,
}

/// Transcribes each span (already padded) of `wav`. Returns the phrases as timed words
/// on the original timeline, and the language Sarvam heard most.
pub async fn transcribe_spans(
    online: &Online,
    wav_path: &Path,
    spans: &[(i64, i64)],
    language: &str,
    work_dir: &Path,
    cancel: &CancellationToken,
    on_progress: impl Fn(f32),
) -> AppResult<Transcript> {
    std::fs::create_dir_all(work_dir)?;
    let info = wav::read_info(wav_path).map_err(|e| AppError::new(ErrorCode::ToolFailed, "The prepared audio couldn't be read.").with_detail(e.to_string()))?;
    let sem = Arc::new(Semaphore::new(PARALLEL));
    let mut set = tokio::task::JoinSet::new();
    for (i, &(start, end)) in spans.iter().enumerate() {
        let path = work_dir.join(format!("o{i:05}.wav"));
        wav::write_slice(wav_path, &info, start, end, &path)?;
        let (sem, online, lang, cancel) = (sem.clone(), online.clone(), language.to_string(), cancel.clone());
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let result = match tokio::fs::read(&path).await {
                Ok(bytes) => request(&online, bytes, &lang, &cancel).await,
                Err(e) => Err(AppError::from(e)),
            };
            let _ = tokio::fs::remove_file(&path).await;
            (i, result)
        });
    }

    let mut results: Vec<Option<Response>> = (0..spans.len()).map(|_| None).collect();
    let mut done = 0;
    on_progress(0.0);
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                set.abort_all();
                return Err(AppError::cancelled());
            }
            next = set.join_next() => next,
        };
        let Some(joined) = next else { break };
        let (i, result) = joined.map_err(|e| AppError::internal(e.to_string()))?;
        match result {
            Ok(r) => results[i] = Some(r),
            Err(e) => {
                set.abort_all();
                return Err(e);
            }
        }
        done += 1;
        on_progress(done as f32 / spans.len().max(1) as f32);
    }

    let mut words = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    for (&(start, end), r) in spans.iter().zip(results.into_iter().flatten()) {
        if let Some(l) = &r.language_code {
            languages.push(from_code(l));
        }
        words.extend(phrases(&r, start, end));
    }
    Ok(Transcript { words, language: majority(&languages), used_gpu: false, repaired_text: false })
}

/// The response's phrases as timed words on the original timeline.
fn phrases(r: &Response, start: i64, end: i64) -> Vec<Word> {
    let word = |s: i64, e: i64, text: &str| Word { start_ms: s.max(start), end_ms: e.min(end).max(s + 1), text: format!(" {}", text.trim()), confidence: 1.0 };
    if let Some(ts) = &r.timestamps {
        let n = ts.words.len().min(ts.start_time_seconds.len()).min(ts.end_time_seconds.len());
        let timed: Vec<Word> = (0..n)
            .filter(|&k| !ts.words[k].trim().is_empty())
            .map(|k| word(start + (ts.start_time_seconds[k] * 1000.0) as i64, start + (ts.end_time_seconds[k] * 1000.0) as i64, &ts.words[k]))
            .collect();
        if !timed.is_empty() {
            return timed;
        }
    }
    if r.transcript.trim().is_empty() {
        Vec::new()
    } else {
        vec![word(start, end, &r.transcript)]
    }
}

fn majority(langs: &[String]) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for l in langs {
        match counts.iter_mut().find(|(k, _)| k == l) {
            Some((_, n)) => *n += 1,
            None => counts.push((l.clone(), 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(l, _)| l)
}

/// One chunk, retried with backoff when Sarvam is busy or the network blips.
async fn request(online: &Online, bytes: Vec<u8>, language: &str, cancel: &CancellationToken) -> AppResult<Response> {
    let mut last = AppError::new(ErrorCode::Network, "Couldn't reach Sarvam. Check your internet connection and try again.");
    let mut attempt = 0;
    let mut limited = 0;
    while attempt < ATTEMPTS {
        if attempt > 0 {
            let wait = Duration::from_millis(800 * 2u64.pow(attempt - 1));
            tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::cancelled()),
                _ = tokio::time::sleep(wait) => {}
            }
        }
        online.slot(cancel).await?;
        let part = reqwest::multipart::Part::bytes(bytes.clone())
            .file_name("speech.wav")
            .mime_str("audio/wav")
            .map_err(|e| AppError::internal(e.to_string()))?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", MODEL)
            .text("mode", "transcribe")
            .text("language_code", language.to_string())
            .text("with_timestamps", "true");
        let sent = online
            .http
            .post(ENDPOINT)
            .header("api-subscription-key", &online.key)
            .multipart(form)
            .timeout(Duration::from_secs(90))
            .send()
            .await;
        let res = match sent {
            Ok(r) => r,
            Err(e) => {
                last = last.clone().with_detail(e.to_string());
                attempt += 1;
                continue;
            }
        };
        let status = res.status().as_u16();
        let body = res.bytes().await.unwrap_or_default();
        if status == 200 {
            return serde_json::from_slice(&body)
                .map_err(|e| AppError::new(ErrorCode::OnlineFailed, "Sarvam sent a reply CaptionNest couldn't read.").with_detail(e.to_string()));
        }
        let err: Option<ErrorInner> = serde_json::from_slice::<ErrorBody>(&body).ok().map(|b| b.error);
        let code = err.as_ref().and_then(|e| e.code.clone()).unwrap_or_default();
        let detail = format!("HTTP {status}: {}", err.as_ref().and_then(|e| e.message.clone()).unwrap_or_default());
        match (status, code.as_str()) {
            (_, "insufficient_quota_error") | (402, _) => {
                return Err(AppError::new(ErrorCode::OnlineCredits, "Your Sarvam credits have run out. Add credits at dashboard.sarvam.ai, or switch to offline recognition in Settings.").with_detail(detail));
            }
            (_, "invalid_api_key_error" | "authentication_error") | (401 | 403, _) => {
                return Err(AppError::new(ErrorCode::OnlineKey, "Sarvam didn't accept the API key. Check it in Settings → Online recognition.").with_detail(detail));
            }
            (429, _) if limited < 40 => {
                // Over the per-minute limit: pace from here on; this isn't a failure.
                limited += 1;
                online.pacer.lock().unwrap().slow_down(std::time::Instant::now());
                last = AppError::new(ErrorCode::OnlineFailed, "Sarvam is busy right now. Try again in a minute.").with_detail(detail);
                continue;
            }
            (429 | 500 | 502 | 503 | 504, _) => {
                last = AppError::new(ErrorCode::OnlineFailed, "Sarvam is busy right now. Try again in a minute.").with_detail(detail);
                attempt += 1;
            }
            _ => {
                return Err(AppError::new(ErrorCode::OnlineFailed, "Sarvam couldn't process this audio.").with_detail(detail));
            }
        }
    }
    Err(last)
}

/// Checks a key with one second of silence (costs a fraction of a paisa).
pub async fn check_key(online: &Online) -> AppResult<()> {
    request(online, wav::silence(1000), "unknown", &CancellationToken::new()).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_round_trip() {
        assert_eq!(language_code("bn"), Some("bn-IN"));
        assert_eq!(language_code("or"), Some("od-IN"));
        assert_eq!(language_code("fr"), None);
        assert_eq!(from_code("bn-IN"), "bn");
        assert_eq!(from_code("od-IN"), "or");
    }

    #[test]
    fn phrases_land_on_the_original_timeline() {
        let r: Response = serde_json::from_str(
            r#"{"request_id":"x","transcript":"এক। দুই।","language_code":"bn-IN",
                "timestamps":{"words":["এক।","দুই।"],"start_time_seconds":[0.0,3.5],"end_time_seconds":[3.2,7.0]}}"#,
        )
        .unwrap();
        let w = phrases(&r, 10_000, 17_500);
        assert_eq!(w.iter().map(|w| (w.start_ms, w.end_ms)).collect::<Vec<_>>(), vec![(10_000, 13_200), (13_500, 17_000)]);
        assert_eq!(w[1].text, " দুই।");
    }

    #[test]
    fn without_timestamps_the_chunk_is_one_phrase() {
        let r: Response = serde_json::from_str(r#"{"transcript":"আমি","language_code":null}"#).unwrap();
        assert_eq!(phrases(&r, 500, 2_000).len(), 1);
        let empty: Response = serde_json::from_str(r#"{"transcript":"  "}"#).unwrap();
        assert!(phrases(&empty, 0, 1).is_empty());
    }

    #[test]
    fn bangla_money_and_digits_read_naturally() {
        assert_eq!(localize("সর্বনিম্ন গ্রেডে ₹17,000। আর সর্বোচ্চ ₹21,500, তবে", "bn"), "সর্বনিম্ন গ্রেডে ১৭,০০০ টাকা। আর সর্বোচ্চ ২১,৫০০ টাকা, তবে");
        assert_eq!(localize("অন্তত 10টি কারখানায় 4,00,000", "bn"), "অন্তত ১০টি কারখানায় ৪,০০,০০০");
        assert_eq!(localize("₹500 कीमत", "hi"), "₹500 कीमत");
    }

    #[test]
    fn requests_are_paced_only_after_sarvam_says_too_many() {
        let mut p = Pacer::default();
        let t0 = std::time::Instant::now();
        assert!((0..500).all(|_| p.take(t0).is_none()));
        p.slow_down(t0);
        let wait = p.take(t0).expect("right after a refusal the next request waits");
        assert!(wait > Duration::from_millis(900) && wait < Duration::from_millis(1300), "{wait:?}");
        assert!(p.take(t0 + Duration::from_secs(2)).is_none());
        assert!(p.take(t0 + Duration::from_secs(2)).is_some());
    }

    #[test]
    fn most_heard_language_wins() {
        assert_eq!(majority(&["bn".into(), "en".into(), "bn".into()]), Some("bn".into()));
        assert_eq!(majority(&[]), None);
    }
}
