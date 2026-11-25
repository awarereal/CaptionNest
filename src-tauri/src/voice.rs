//! Voice typing: speech recorded from the microphone, written into a caption by Deepgram.
//!
//! The web view records the speech and hands the whole recording to this module, which
//! sends it to Deepgram's pre-recorded API with Nova-3. Deepgram added Bengali (`bn`) to
//! Nova-3 in January 2026 and released an improved Bengali model in March 2026; the
//! monolingual model is used because the speaker is mainly Bangla and the caption must
//! stay in Bengali script. `smart_format` adds punctuation. Pre-recorded (not streaming)
//! recognition hears the whole utterance at once, which favours accuracy over speed.
//!
//! The key is kept in its own file in the app's config folder. It is never sent to the
//! web view (the interface only learns whether a key is saved, plus a masked hint) and
//! never logged.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::asr::sarvam;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::wav;

const LISTEN: &str = "https://api.deepgram.com/v1/listen";
const MODEL: &str = "nova-3";
const FILE: &str = "voice.json";
/// Largest recording accepted (the interface stops at 5 minutes, about 3 MB of Opus).
const MAX_BYTES: usize = 60 * 1024 * 1024;
const ATTEMPTS: u32 = 3;

/// Deepgram's language code for a caption language voice typing supports.
pub fn language_code(lang: &str) -> Option<&'static str> {
    match lang {
        "bn" => Some("bn"),
        "en" => Some("en"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub key_saved: bool,
    /// The first and last characters of the key, so the user can recognize it.
    pub key_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heard {
    pub text: String,
    pub confidence: f32,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(rename = "deepgramApiKey")]
    key: Option<String>,
}

/// The saved Deepgram key. Debug output never shows it.
pub struct VoiceKey {
    file: PathBuf,
    key: Mutex<Option<String>>,
}

impl std::fmt::Debug for VoiceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VoiceKey { key: <hidden> }")
    }
}

impl VoiceKey {
    pub fn load(config_dir: &Path) -> Self {
        let file = config_dir.join(FILE);
        let key = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Stored>(&b).ok())
            .and_then(|s| s.key)
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        Self { file, key: Mutex::new(key) }
    }

    pub fn get(&self) -> Option<String> {
        self.key.lock().unwrap().clone()
    }

    pub fn status(&self) -> Status {
        let key = self.key.lock().unwrap();
        Status { key_saved: key.is_some(), key_hint: key.as_deref().map(mask) }
    }

    pub fn set(&self, key: Option<String>) -> AppResult<()> {
        let key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        match &key {
            Some(_) => {
                let dir = self.file.parent().unwrap_or(Path::new("."));
                std::fs::create_dir_all(dir)?;
                let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
                std::io::Write::write_all(&mut tmp, &serde_json::to_vec_pretty(&Stored { key: key.clone() })?)?;
                tmp.as_file().sync_all()?;
                crate::project::persist(tmp, &self.file)?;
            }
            None => {
                if let Err(e) = std::fs::remove_file(&self.file) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(e.into());
                    }
                }
            }
        }
        *self.key.lock().unwrap() = key;
        Ok(())
    }
}

/// "a1b2c…9f0e": enough to recognize a key without showing it.
pub(crate) fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 10 {
        return "••••".into();
    }
    let head: String = chars[..5].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// Checks a key the way voice typing uses it: one second of silence through Nova-3 in
/// Bengali. This also confirms the key's project may use the Bengali model.
pub async fn check_key(http: &reqwest::Client, key: &str) -> AppResult<()> {
    listen(http, key, wav::silence(1000), "audio/wav", "bn").await.map(|_| ())
}

/// Recognizes one recording. `mime` is the recorder's container type.
pub async fn transcribe(http: &reqwest::Client, key: &str, audio: Vec<u8>, mime: &str, lang: &str) -> AppResult<Heard> {
    if audio.len() < 64 {
        return Err(no_words());
    }
    if audio.len() > MAX_BYTES {
        return Err(AppError::new(ErrorCode::VoiceFailed, "That recording is too long. Record up to 5 minutes at a time."));
    }
    let heard = listen(http, key, audio, mime, lang).await?;
    if heard.text.trim().is_empty() {
        return Err(no_words());
    }
    Ok(Heard { text: sarvam::localize(heard.text.trim(), lang), confidence: heard.confidence })
}

fn no_words() -> AppError {
    AppError::new(ErrorCode::NoSpeech, "No words were heard. Try again a little closer to the microphone, with less background noise.")
}

/// Only the recorder's container types are forwarded; Deepgram detects the codec itself.
fn content_type(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "audio/webm" | "video/webm" => "audio/webm",
        "audio/mp4" | "video/mp4" | "audio/x-m4a" | "audio/aac" => "audio/mp4",
        "audio/ogg" => "audio/ogg",
        "audio/wav" | "audio/wave" | "audio/x-wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize)]
struct Response {
    results: Results,
}

#[derive(Deserialize)]
struct Results {
    channels: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
    alternatives: Vec<Alternative>,
}

#[derive(Deserialize)]
struct Alternative {
    #[serde(default)]
    transcript: String,
    #[serde(default)]
    confidence: f32,
}

fn parse(body: &[u8]) -> AppResult<Heard> {
    let r: Response = serde_json::from_slice(body)
        .map_err(|e| AppError::new(ErrorCode::VoiceFailed, "Deepgram sent a reply CaptionNest couldn't read.").with_detail(e.to_string()))?;
    let best = r.results.channels.into_iter().next().and_then(|c| c.alternatives.into_iter().next());
    Ok(best.map(|a| Heard { text: a.transcript, confidence: a.confidence }).unwrap_or(Heard { text: String::new(), confidence: 0.0 }))
}

/// Maps a Deepgram error reply to a plain message. `None` means "worth retrying".
fn classify(status: u16, body: &[u8]) -> (Option<AppError>, String) {
    let v: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let code = v.get("err_code").or_else(|| v.get("category")).and_then(|c| c.as_str()).unwrap_or_default().to_string();
    let msg = v.get("err_msg").or_else(|| v.get("message")).and_then(|c| c.as_str()).unwrap_or_default();
    let detail = format!("HTTP {status} {code}: {msg}");
    let err = match status {
        401 => Some(AppError::new(ErrorCode::VoiceKey, "Deepgram didn't accept the API key. Check it in Settings → Voice typing.")),
        403 => Some(AppError::new(
            ErrorCode::VoiceKey,
            "This Deepgram key isn't allowed to use the speech model. Check the key's permissions in the Deepgram console.",
        )),
        402 => Some(AppError::new(ErrorCode::VoiceCredits, "Your Deepgram credits have run out. Add credits in the Deepgram console, then try again.")),
        429 | 500..=599 => None,
        400 | 413 | 415 | 422 => Some(AppError::new(ErrorCode::VoiceFailed, "The recording couldn't be read. Please record again.")),
        _ => Some(AppError::new(ErrorCode::VoiceFailed, "Deepgram couldn't turn the recording into text.")),
    };
    (err.map(|e| e.with_detail(detail.clone())), detail)
}

async fn listen(http: &reqwest::Client, key: &str, audio: Vec<u8>, mime: &str, lang: &str) -> AppResult<Heard> {
    if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(AppError::new(ErrorCode::VoiceKey, "The Deepgram key contains spaces or line breaks. Copy it again from the Deepgram console."));
    }
    let Some(language) = language_code(lang) else {
        return Err(AppError::new(ErrorCode::VoiceFailed, "Voice typing works in Bangla and English captions."));
    };
    // Every value is a fixed ASCII word, so the query needs no escaping.
    let url = format!("{LISTEN}?model={MODEL}&language={language}&smart_format=true&punctuate=true");
    let mut last = AppError::new(ErrorCode::Network, "Couldn't reach Deepgram. Check your internet connection and try again.");
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * 2u64.pow(attempt - 1))).await;
        }
        let sent = http
            .post(&url)
            .header("Authorization", format!("Token {key}"))
            .header("Content-Type", content_type(mime))
            .body(audio.clone())
            .timeout(Duration::from_secs(120))
            .send()
            .await;
        let res = match sent {
            Ok(r) => r,
            Err(e) => {
                last = AppError::new(ErrorCode::Network, "Couldn't reach Deepgram. Check your internet connection and try again.").with_detail(e.to_string());
                continue;
            }
        };
        let status = res.status().as_u16();
        let body = res.bytes().await.unwrap_or_default();
        if status == 200 {
            return parse(&body);
        }
        match classify(status, &body) {
            (Some(err), _) => return Err(err),
            (None, detail) => last = AppError::new(ErrorCode::VoiceFailed, "Deepgram is busy right now. Try again in a moment.").with_detail(detail),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_best_transcript() {
        let body = r#"{"metadata":{},"results":{"channels":[{"alternatives":[{"transcript":"আমি ভালো আছি।","confidence":0.93,"words":[]}]}]}}"#;
        let h = parse(body.as_bytes()).unwrap();
        assert_eq!(h.text, "আমি ভালো আছি।");
        assert!((h.confidence - 0.93).abs() < 1e-6);
        let empty = parse(br#"{"results":{"channels":[{"alternatives":[]}]}}"#).unwrap();
        assert_eq!(empty.text, "");
    }

    #[test]
    fn errors_become_plain_messages() {
        let (e, _) = classify(401, br#"{"err_code":"INVALID_AUTH","err_msg":"Invalid credentials.","request_id":"x"}"#);
        assert_eq!(e.unwrap().code, ErrorCode::VoiceKey);
        let (e, _) = classify(402, br#"{"err_code":"ASR_PAYMENT_REQUIRED","err_msg":"..."}"#);
        assert_eq!(e.unwrap().code, ErrorCode::VoiceCredits);
        assert!(classify(429, b"").0.is_none(), "rate limits are retried");
        assert!(classify(503, b"not json").0.is_none());
        assert_eq!(classify(400, b"{}").0.unwrap().code, ErrorCode::VoiceFailed);
    }

    #[test]
    fn only_container_types_are_forwarded() {
        assert_eq!(content_type("audio/webm;codecs=opus"), "audio/webm");
        assert_eq!(content_type("audio/mp4"), "audio/mp4");
        assert_eq!(content_type("text/html"), "application/octet-stream");
    }

    #[test]
    fn key_is_saved_masked_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let k = VoiceKey::load(dir.path());
        assert!(!k.status().key_saved);
        k.set(Some("  dg_0123456789abcdef  ".into())).unwrap();
        assert_eq!(k.get().as_deref(), Some("dg_0123456789abcdef"));
        assert_eq!(k.status().key_hint.as_deref(), Some("dg_01…cdef"));
        assert!(!format!("{k:?}").contains("dg_0"), "debug output must not show the key");
        let again = VoiceKey::load(dir.path());
        assert_eq!(again.get().as_deref(), Some("dg_0123456789abcdef"));
        again.set(None).unwrap();
        assert!(!VoiceKey::load(dir.path()).status().key_saved);
    }

    /// Live check against Deepgram, the way the app calls it. Needs DEEPGRAM_API_KEY and
    /// CAPTIONNEST_VOICE_SAMPLES (comma-separated Bangla recordings: .wav, .webm, .m4a).
    #[tokio::test]
    #[ignore]
    async fn live_bangla_transcription() {
        let (Ok(key), Ok(samples)) = (std::env::var("DEEPGRAM_API_KEY"), std::env::var("CAPTIONNEST_VOICE_SAMPLES")) else { return };
        let http = crate::models::download::client();
        check_key(&http, &key).await.expect("key accepted");
        assert_eq!(check_key(&http, "0000000000000000000000000000000000000000").await.unwrap_err().code, ErrorCode::VoiceKey);
        for path in samples.split(',') {
            let mime = match path.rsplit('.').next() {
                Some("webm") => "audio/webm;codecs=opus",
                Some("m4a") => "audio/mp4",
                _ => "audio/wav",
            };
            let heard = transcribe(&http, &key, std::fs::read(path).unwrap(), mime, "bn").await.expect(path);
            let letters: Vec<char> = heard.text.chars().filter(|c| c.is_alphabetic()).collect();
            let bengali = letters.iter().filter(|c| ('\u{0980}'..='\u{09FF}').contains(*c)).count();
            println!("{mime}: {:.2} {}", heard.confidence, heard.text);
            assert!(bengali * 10 >= letters.len() * 9, "mostly Bengali script: {}", heard.text);
        }
    }

    #[test]
    fn languages_are_limited_to_supported_ones() {
        assert_eq!(language_code("bn"), Some("bn"));
        assert_eq!(language_code("fr"), None);
    }
}
