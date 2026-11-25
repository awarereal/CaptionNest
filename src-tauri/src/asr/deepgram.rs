//! Online recognition for captions with Deepgram's Nova-3 (the HTTP call is in
//! `voice.rs`, shared with voice typing).
//!
//! Sarvam returns one time span per request, so its words have to be placed between
//! pauses. Deepgram times every word itself, and those times hold up where pauses found
//! from the sound don't: on a 12-minute vlog with music and several voices, captions
//! placed between pauses started within 150 ms of Deepgram's word times only 26% of
//! the time, and 10% were 1.8 s or more off.
//!
//! Measured on the project's Bangla test material (November 2026):
//! - exact-timing phrase recording (`scripts/bench/make_phrase_audio.py`): Deepgram's first
//!   word of a phrase starts a median 80 ms from the true start, 69% within 150 ms.
//! - the 5-minute FLEURS clip used for `scripts/bench/bn_sarvam.py`: 16.0% word errors
//!   and 5.9% letter errors (Sarvam saaras:v4: 13.1% and 4.0%; best offline model 26.5%).
//! - its own language detection called a Bangla vlog Spanish, so it's always told the
//!   language.
//!
//! The whole sound track is sent, not only the speech the voice-activity model found, so
//! speech under music isn't skipped. Long videos go in pieces of at most five minutes,
//! cut in a pause, three at a time.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::whisper::Transcript;
use super::{sarvam, Word};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::wav;
use crate::voice::{self, Deepgram};

pub const MODEL: &str = "nova-3";
/// Longest piece sent in one request.
const PIECE_MS: i64 = 300_000;
/// How far before the five-minute mark to look for a pause to cut in.
const CUT_SEARCH_MS: i64 = 60_000;
/// Requests at a time. Most are short pieces between pauses (timing for Sarvam's text).
const PARALLEL: usize = 5;

/// Splits `0..total_ms` into pieces of at most five minutes, each cut in the middle of
/// the longest pause in the minute before the limit (or at the limit when there's none).
pub fn plan_pieces(total_ms: i64, speech: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    while total_ms - start > PIECE_MS {
        let limit = start + PIECE_MS;
        let cut = speech
            .windows(2)
            .map(|w| (w[0].1, w[1].0))
            .filter(|&(a, b)| b > a && a >= limit - CUT_SEARCH_MS && b <= limit)
            .max_by_key(|&(a, b)| b - a)
            .map_or(limit, |(a, b)| (a + b) / 2);
        pieces.push((start, cut));
        start = cut;
    }
    pieces.push((start, total_ms.max(start)));
    pieces
}

/// Merges overlapping spans (several phrases can share one chunk) into sorted pieces.
pub fn merge_spans(spans: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = spans.iter().copied().filter(|&(a, b)| b > a).collect();
    v.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a < last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Recognizes each piece of `wav` (16 kHz mono) in `language` (a Deepgram code), one
/// request per piece. Returns every word on the video's timeline.
pub async fn transcribe_spans(
    dg: &Deepgram,
    wav_path: &Path,
    pieces: &[(i64, i64)],
    language: &str,
    work_dir: &Path,
    cancel: &CancellationToken,
    on_progress: impl Fn(f32),
) -> AppResult<Transcript> {
    std::fs::create_dir_all(work_dir)?;
    let info = wav::read_info(wav_path).map_err(|e| AppError::new(ErrorCode::ToolFailed, "The prepared audio couldn't be read.").with_detail(e.to_string()))?;
    let sem = Arc::new(Semaphore::new(PARALLEL));
    let mut set = tokio::task::JoinSet::new();
    for (i, &(start, end)) in pieces.iter().enumerate() {
        let path = work_dir.join(format!("d{i:04}.wav"));
        wav::write_slice(wav_path, &info, start, end, &path)?;
        let (sem, dg, lang) = (sem.clone(), dg.clone(), language.to_string());
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let result = match tokio::fs::read(&path).await {
                Ok(bytes) => voice::recognize_words(&dg, bytes, &lang).await,
                Err(e) => Err(AppError::from(e)),
            };
            let _ = tokio::fs::remove_file(&path).await;
            (i, result)
        });
    }

    let mut results: Vec<Vec<voice::TimedWord>> = vec![Vec::new(); pieces.len()];
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
            Ok(w) => results[i] = w,
            Err(e) => {
                set.abort_all();
                return Err(e);
            }
        }
        done += 1;
        on_progress(done as f32 / pieces.len().max(1) as f32);
    }

    let lang = from_code(language);
    let words = pieces.iter().zip(results).flat_map(|(&(start, end), heard)| to_words(heard, start, end, &lang)).collect();
    Ok(Transcript { words, language: Some(lang), used_gpu: false, repaired_text: false })
}

/// Longest a word lasts, from its letters: slow speech (10 letters a second) plus a
/// little. Deepgram stretches the last word before a pause: "ছিল।" ran 0.8 s where the
/// voice stopped after 0.34 s, so the caption stayed up into the pause.
fn longest_ms(text: &str) -> i64 {
    super::align::letters(text).len() as i64 * 100 + 150
}

/// Deepgram's words of one piece starting at `start` ms, on the video's timeline.
fn to_words(heard: Vec<voice::TimedWord>, start: i64, end: i64, lang: &str) -> Vec<Word> {
    heard
        .into_iter()
        .filter(|w| !w.text.trim().is_empty())
        .map(|w| {
            let s = (start + w.start_ms).min(end);
            Word {
                start_ms: s,
                end_ms: (start + w.end_ms).min(s + longest_ms(&w.text)).clamp(s + 1, end.max(s + 1)),
                text: format!(" {}", sarvam::localize(w.text.trim(), lang)),
                confidence: w.confidence,
            }
        })
        .collect()
}

/// CaptionNest's language code for a Deepgram one (they're the same two letters here).
fn from_code(code: &str) -> String {
    code.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_audio_is_one_piece() {
        assert_eq!(plan_pieces(120_000, &[(1_000, 5_000)]), vec![(0, 120_000)]);
    }

    #[test]
    fn long_audio_is_cut_in_the_longest_late_pause() {
        let speech = [(0, 250_000), (251_000, 280_000), (283_000, 299_500), (300_500, 700_000)];
        let p = plan_pieces(700_000, &speech);
        // The 3 s pause (280-283 s) wins over the 1 s ones.
        assert_eq!(p[0], (0, 281_500));
        assert_eq!(p[1].0, 281_500);
        assert!(p.iter().all(|&(a, b)| b - a <= PIECE_MS && b > a));
        assert_eq!(p.last().unwrap().1, 700_000);
    }

    #[test]
    fn no_pause_cuts_at_the_limit() {
        let p = plan_pieces(650_000, &[(0, 650_000)]);
        assert_eq!(p, vec![(0, 300_000), (300_000, 600_000), (600_000, 650_000)]);
    }

    #[test]
    fn shared_chunks_are_sent_once() {
        assert_eq!(
            merge_spans(&[(5_000, 9_000), (0, 4_000), (5_000, 9_000), (8_000, 12_000), (12_000, 15_000)]),
            vec![(0, 4_000), (5_000, 12_000), (12_000, 15_000)]
        );
    }

    #[test]
    fn words_move_to_the_video_timeline() {
        let heard = vec![
            voice::TimedWord { start_ms: 480, end_ms: 800, text: "আমি".into(), confidence: 0.9 },
            voice::TimedWord { start_ms: 800, end_ms: 800, text: "আছি।".into(), confidence: 0.4 },
        ];
        let w = to_words(heard, 300_000, 600_000, "bn");
        assert_eq!((w[0].start_ms, w[0].end_ms, w[0].text.as_str()), (300_480, 300_800, " আমি"));
        // A zero-length word still lasts a millisecond.
        assert_eq!((w[1].start_ms, w[1].end_ms), (300_800, 300_801));
        // A stretched last word is cut to a believable length (3 letters: 0.45 s).
        let long = vec![voice::TimedWord { start_ms: 3_600, end_ms: 4_400, text: "ছিল।".into(), confidence: 0.9 }];
        assert_eq!(to_words(long, 0, 10_000, "bn")[0].end_ms, 4_050);
    }
}
