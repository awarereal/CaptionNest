//! Live voice typing: the microphone's sound streams to Deepgram while you speak and the
//! words come back as you say them (a running guess, then final words at each short
//! pause), instead of all at once after "Stop".
//!
//! The web view can't open network connections (the CSP allows only IPC), so the sound
//! goes through here: `start` opens Deepgram's streaming endpoint, `audio` forwards raw
//! 16-bit mono PCM, `finish` asks Deepgram for its last words and closes. Results arrive
//! in the window as `voice-live` events. The key never leaves Rust.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::voice;

const LISTEN: &str = "wss://api.deepgram.com/v1/listen";
/// Deepgram closes a stream after 10 s without data; quiet stretches get a KeepAlive.
const KEEPALIVE: Duration = Duration::from_secs(4);
/// After "finish", how long to wait for Deepgram's last words.
const FINISH_WAIT: Duration = Duration::from_secs(6);
const CONNECT_WAIT: Duration = Duration::from_secs(10);

enum In {
    Audio(Vec<u8>),
    Finish,
}

struct Session {
    id: u64,
    tx: mpsc::UnboundedSender<In>,
    task: tokio::task::JoinHandle<()>,
    order: Order,
}

/// Puts the sound back in order: each piece is its own IPC call, and calls can overtake
/// each other.
#[derive(Default)]
struct Order {
    next: u64,
    held: std::collections::BTreeMap<u64, Vec<u8>>,
}

impl Order {
    /// Piece `seq` arrived; returns the pieces that can go out now, in order.
    fn take(&mut self, seq: u64, pcm: Vec<u8>) -> Vec<Vec<u8>> {
        if seq < self.next {
            return Vec::new();
        }
        self.held.insert(seq, pcm);
        // A piece that never comes (a failed call) mustn't hold everything back.
        if self.held.len() > 16 {
            self.next = *self.held.keys().next().unwrap_or(&self.next);
        }
        let mut out = Vec::new();
        while let Some(p) = self.held.remove(&self.next) {
            out.push(p);
            self.next += 1;
        }
        out
    }
}

/// The one live dictation at a time.
#[derive(Default)]
pub struct Live {
    current: Mutex<Option<Session>>,
    next_id: AtomicU64,
}

/// What the window hears about a live dictation. `kind`: "words" (a running guess, or
/// final words when `is_final`), "end" (nothing more will come) or "error".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveEvent {
    pub id: u64,
    pub kind: &'static str,
    pub text: String,
    pub is_final: bool,
    pub error: Option<AppError>,
}

impl Live {
    /// Connects to Deepgram for a new dictation (ending any earlier one) and returns its id.
    pub async fn start(&self, app: AppHandle, key: &str, language: &str, sample_rate: u32) -> AppResult<u64> {
        self.stop_current();
        let Some(lang) = voice::language_code(language) else {
            return Err(AppError::new(ErrorCode::VoiceFailed, "Voice typing works in Bangla and English captions."));
        };
        let url = live_url(lang, sample_rate);
        let mut req = url.into_client_request().map_err(|e| AppError::internal(e.to_string()))?;
        let auth = HeaderValue::from_str(&format!("Token {key}"))
            .map_err(|_| AppError::new(ErrorCode::VoiceKey, "The Deepgram key contains characters that can't be sent. Copy it again from the Deepgram console."))?;
        req.headers_mut().insert("Authorization", auth);
        let (ws, _) = tokio::time::timeout(CONNECT_WAIT, tokio_tungstenite::connect_async(req))
            .await
            .map_err(|_| AppError::new(ErrorCode::Network, "Deepgram didn't answer. Check your internet connection and try again."))?
            .map_err(connect_error)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(app, id, ws, rx));
        *self.current.lock().unwrap() = Some(Session { id, tx, task, order: Order::default() });
        Ok(id)
    }

    /// Piece `seq` (0, 1, 2, …) of the sound for dictation `id`: 16-bit little-endian mono
    /// PCM at the rate given to start.
    pub fn audio(&self, id: u64, seq: u64, pcm: Vec<u8>) {
        if let Some(s) = self.current.lock().unwrap().as_mut().filter(|s| s.id == id) {
            for piece in s.order.take(seq, pcm) {
                let _ = s.tx.send(In::Audio(piece));
            }
        }
    }

    /// Asks for the last words; an "end" event follows.
    pub fn finish(&self, id: u64) {
        if let Some(s) = self.current.lock().unwrap().as_ref().filter(|s| s.id == id) {
            let _ = s.tx.send(In::Finish);
        }
    }

    /// Drops dictation `id` at once; no more events come for it.
    pub fn cancel(&self, id: u64) {
        let mut cur = self.current.lock().unwrap();
        if let Some(s) = cur.take_if(|s| s.id == id) {
            s.task.abort();
        }
    }

    fn stop_current(&self) {
        if let Some(s) = self.current.lock().unwrap().take() {
            s.task.abort();
        }
    }
}

fn live_url(lang: &str, sample_rate: u32) -> String {
    // endpointing: a pause of 0.3 s makes the words so far final, so they can be written
    // into the caption while you keep speaking. Every value is a fixed ASCII word or a number.
    format!(
        "{LISTEN}?model=nova-3&language={lang}&encoding=linear16&sample_rate={sample_rate}&channels=1\
         &interim_results=true&smart_format=true&punctuate=true&endpointing=300"
    )
}

/// A failed connection, in plain words.
fn connect_error(e: tungstenite::Error) -> AppError {
    match e {
        tungstenite::Error::Http(resp) => {
            let status = resp.status().as_u16();
            let detail = format!("HTTP {status} {}", resp.body().as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default());
            let err = match status {
                401 => AppError::new(ErrorCode::VoiceKey, "Deepgram didn't accept the API key. Check it in Settings → Voice typing."),
                403 => AppError::new(ErrorCode::VoiceKey, "This Deepgram key isn't allowed to use live speech recognition. Check the key's permissions in the Deepgram console."),
                402 => AppError::new(ErrorCode::VoiceCredits, "Your Deepgram credits have run out. Add credits in the Deepgram console, then try again."),
                429 => AppError::new(ErrorCode::VoiceFailed, "Deepgram is busy right now. Wait a moment and try again."),
                _ => AppError::new(ErrorCode::VoiceFailed, "Deepgram couldn't start listening. Try again."),
            };
            err.with_detail(detail)
        }
        other => AppError::new(ErrorCode::Network, "Couldn't reach Deepgram. Check your internet connection and try again.").with_detail(other.to_string()),
    }
}

#[derive(Deserialize)]
struct LiveResult {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    is_final: bool,
    channel: Option<LiveChannel>,
}

#[derive(Deserialize)]
struct LiveChannel {
    #[serde(default)]
    alternatives: Vec<LiveAlternative>,
}

#[derive(Deserialize)]
struct LiveAlternative {
    #[serde(default)]
    transcript: String,
}

/// The words in one Deepgram message, and whether they're final. Other messages
/// (metadata, speech-started notices) give None.
fn parse(text: &str) -> Option<(String, bool)> {
    let r: LiveResult = serde_json::from_str(text).ok()?;
    if r.kind != "Results" {
        return None;
    }
    let words = r.channel?.alternatives.into_iter().next()?.transcript;
    Some((words, r.is_final))
}

async fn run(app: AppHandle, id: u64, ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, mut rx: mpsc::UnboundedReceiver<In>) {
    let emit = |kind: &'static str, text: String, is_final: bool, error: Option<AppError>| {
        let _ = app.emit("voice-live", LiveEvent { id, kind, text, is_final, error });
    };
    let (mut sink, mut stream) = ws.split();
    let mut keepalive = tokio::time::interval(KEEPALIVE);
    let mut last_sent = Instant::now();
    let mut finishing: Option<Instant> = None;
    loop {
        let deadline = finishing.map_or_else(|| Instant::now() + Duration::from_secs(3600), |t| t + FINISH_WAIT);
        tokio::select! {
            input = rx.recv(), if finishing.is_none() => match input {
                Some(In::Audio(pcm)) => {
                    if let Err(e) = sink.send(Message::Binary(pcm.into())).await {
                        emit("error", String::new(), false, Some(lost(e)));
                        break;
                    }
                    last_sent = Instant::now();
                }
                Some(In::Finish) | None => {
                    let _ = sink.send(Message::Text(r#"{"type":"CloseStream"}"#.into())).await;
                    finishing = Some(Instant::now());
                }
            },
            _ = keepalive.tick(), if finishing.is_none() => {
                if last_sent.elapsed() >= KEEPALIVE {
                    let _ = sink.send(Message::Text(r#"{"type":"KeepAlive"}"#.into())).await;
                }
            }
            _ = tokio::time::sleep_until(deadline), if finishing.is_some() => break,
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if let Some((words, is_final)) = parse(&t) {
                        emit("words", words, is_final, None);
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    // Deepgram closes with 1000 after CloseStream; anything else before
                    // "finish" means the stream broke (e.g. no data for 10 s).
                    let normal = frame.as_ref().is_none_or(|f| u16::from(f.code) == 1000);
                    if finishing.is_none() && !normal {
                        let why = frame.map(|f| format!("{} {}", u16::from(f.code), f.reason)).unwrap_or_default();
                        emit("error", String::new(), false, Some(AppError::new(ErrorCode::VoiceFailed, "Deepgram stopped listening. Try again.").with_detail(why)));
                    }
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    if finishing.is_none() {
                        emit("error", String::new(), false, Some(lost(e)));
                    }
                    break;
                }
                None => break,
            },
        }
    }
    emit("end", String::new(), false, None);
}

fn lost(e: tungstenite::Error) -> AppError {
    AppError::new(ErrorCode::Network, "The connection to Deepgram was lost. Check your internet connection and try again.").with_detail(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_running_and_final_words() {
        let interim = r#"{"type":"Results","is_final":false,"speech_final":false,"channel":{"alternatives":[{"transcript":"আমি আজকে","confidence":0.8}]}}"#;
        let fin = r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"আমি আজকে বাজারে যাব।"}]}}"#;
        assert_eq!(parse(interim), Some(("আমি আজকে".to_string(), false)));
        assert_eq!(parse(fin), Some(("আমি আজকে বাজারে যাব।".to_string(), true)));
        assert_eq!(parse(r#"{"type":"Metadata","request_id":"x"}"#), None);
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn sound_goes_out_in_order() {
        let mut o = Order::default();
        assert_eq!(o.take(1, vec![1]), Vec::<Vec<u8>>::new());
        assert_eq!(o.take(0, vec![0]), vec![vec![0], vec![1]]);
        assert_eq!(o.take(1, vec![9]), Vec::<Vec<u8>>::new(), "a repeat is dropped");
        // Piece 2 never arrives: after 16 more, the rest goes on without it.
        for k in 3..19 {
            assert!(o.take(k, vec![k as u8]).is_empty());
        }
        let out = o.take(19, vec![19]);
        assert_eq!(out.len(), 17);
        assert_eq!(out[0], vec![3]);
    }

    #[test]
    fn asks_for_running_words_at_the_microphone_rate() {
        let u = live_url("bn", 16_000);
        assert!(u.starts_with("wss://api.deepgram.com/v1/listen?model=nova-3&language=bn&encoding=linear16&sample_rate=16000"));
        assert!(u.contains("interim_results=true") && u.contains("endpointing=300"));
    }

    /// Streams real speech to Deepgram at speaking pace and prints what comes back.
    /// DEEPGRAM_API_KEY=… CAPTIONNEST_VOICE_SAMPLES=a.wav (16 kHz mono 16-bit)
    /// cargo test --lib live_streaming_words -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_streaming_words() {
        use tokio_tungstenite::connect_async;
        let key = std::env::var("DEEPGRAM_API_KEY").expect("DEEPGRAM_API_KEY");
        let file = std::env::var("CAPTIONNEST_VOICE_SAMPLES").expect("CAPTIONNEST_VOICE_SAMPLES");
        let wav = std::fs::read(file.split(',').next().unwrap()).unwrap();
        let pieces: Vec<Vec<u8>> = wav[44..].chunks(3_200).map(<[u8]>::to_vec).collect();
        let mut req = live_url("bn", 16_000).into_client_request().unwrap();
        req.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Token {key}")).unwrap());
        let (ws, _) = connect_async(req).await.expect("connect");
        let (mut sink, mut stream) = ws.split();
        let started = std::time::Instant::now();
        let sender = tokio::spawn(async move {
            // 100 ms pieces at speaking pace, like the microphone.
            for piece in pieces {
                sink.send(Message::Binary(piece.into())).await.unwrap();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            sink.send(Message::Text(r#"{"type":"CloseStream"}"#.into())).await.unwrap();
        });
        let mut finals = Vec::new();
        let mut first_words_ms = None;
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(t) = msg {
                if let Some((words, is_final)) = parse(&t) {
                    if !words.is_empty() && first_words_ms.is_none() {
                        first_words_ms = Some(started.elapsed().as_millis());
                    }
                    println!("{:>6} ms {} {words}", started.elapsed().as_millis(), if is_final { "FINAL" } else { "     " });
                    if is_final && !words.is_empty() {
                        finals.push(words);
                    }
                }
            }
        }
        sender.await.unwrap();
        println!("first words after {first_words_ms:?} ms; final text: {}", finals.join(" "));
        assert!(!finals.is_empty(), "Deepgram heard nothing");
    }
}
