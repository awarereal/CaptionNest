//! Voice activity detection with the bundled Silero model (whisper.cpp's
//! `vad-speech-segments` tool, shipped as `whisper-vad`).
//!
//! CaptionNest runs VAD as a separate step instead of whisper-cli's built-in `--vad`:
//! in tests the built-in mode made the Fast model stop after the first sentence and
//! produced zero-length duplicate segments. Running it separately keeps full recall
//! while still letting us
//!   - report "no speech" quickly for silent or music-only files,
//!   - start language detection at the first real speech, and
//!   - drop words Whisper invents during silence or music.

use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::Word;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::media::process;

/// Speech regions in milliseconds on the original timeline.
pub type Segments = Vec<(i64, i64)>;

/// Padding the VAD adds on each side of a speech region (`-vp`).
pub const PAD_MS: i64 = 120;

/// Where speech really starts and stops: the regions without their padding.
pub fn unpadded(speech: &[(i64, i64)]) -> Segments {
    speech
        .iter()
        .map(|&(s, e)| {
            let mid = (s + e) / 2;
            let a = if s == 0 { 0 } else { (s + PAD_MS).min(mid) };
            (a, (e - PAD_MS).max(mid).max(a))
        })
        .collect()
}

pub async fn speech_segments(tool: &Path, wav: &Path, model: &Path, threads: usize, cancel: &CancellationToken) -> AppResult<Segments> {
    let mut cmd = process::command(tool);
    cmd.arg("-f").arg(wav).arg("-vm").arg(model)
        .args(["-t", &threads.to_string(), "-np"])
        .args(["-vt", "0.5", "-vspd", "200", "-vsd", "300", "-vp", &PAD_MS.to_string(), "-vmsd", "10"]);
    let mut segments = Vec::new();
    let out = process::run_streaming(cmd, cancel, false, |line| {
        if let Some(s) = parse_line(line) {
            segments.push(s);
        }
    }, |_| {})
    .await?;
    if !out.status.success() {
        return Err(AppError::new(ErrorCode::ToolFailed, "Speech detection stopped unexpectedly.").with_detail(out.stderr_tail));
    }
    Ok(segments)
}

/// "Speech segment 3: start = 1283.00, end = 1440.00" (centiseconds) → (12830, 14400)
fn parse_line(line: &str) -> Option<(i64, i64)> {
    let rest = line.trim().strip_prefix("Speech segment ")?;
    let (_, times) = rest.split_once(':')?;
    let mut start = None;
    let mut end = None;
    for part in times.split(',') {
        let (k, v) = part.split_once('=')?;
        let cs: f64 = v.trim().parse().ok()?;
        match k.trim() {
            "start" => start = Some((cs * 10.0).round() as i64),
            "end" => end = Some((cs * 10.0).round() as i64),
            _ => {}
        }
    }
    let (s, e) = (start?, end?);
    (e > s).then_some((s, e))
}

/// Keeps words that overlap speech (each region widened by `pad_ms`).
pub fn keep_speech(words: Vec<Word>, speech: &[(i64, i64)], pad_ms: i64) -> Vec<Word> {
    if speech.is_empty() {
        return words;
    }
    words
        .into_iter()
        .filter(|w| {
            let (a, b) = (w.start_ms, w.end_ms.max(w.start_ms + 1));
            speech.iter().any(|&(s, e)| a < e + pad_ms && b > s - pad_ms)
        })
        .collect()
}

/// Longest piece of audio recognized at once. Whisper writes at most ~224 tokens per
/// 30-second window; scripts such as Bengali, Devanagari or Arabic need many tokens per
/// word, so long stretches get cut off. On CaptionNest's Bangla test, 12-second chunks
/// cut the error rate from 54% to 24% and recovered dropped sentence endings.
pub fn chunk_limit_ms(lang: Option<&str>) -> i64 {
    match lang {
        Some(l) if is_compact_script(l) => 24_000,
        _ => 12_000,
    }
}

/// Languages written in Latin, Cyrillic or Greek script, which tokenize compactly.
fn is_compact_script(l: &str) -> bool {
    matches!(
        l,
        "en" | "es" | "fr" | "de" | "it" | "pt" | "nl" | "sv" | "da" | "no" | "nn" | "fi" | "pl" | "cs" | "sk" | "sl" | "hr"
            | "bs" | "ro" | "hu" | "et" | "lv" | "lt" | "ca" | "gl" | "eu" | "id" | "ms" | "tl" | "vi" | "tr" | "az" | "sw"
            | "af" | "cy" | "ga" | "is" | "mt" | "lb" | "fo" | "oc" | "br" | "la" | "sq" | "uz" | "tk" | "ht" | "jw" | "su"
            | "so" | "yo" | "ha" | "sn" | "ln" | "mg" | "mi" | "haw" | "ru" | "uk" | "be" | "bg" | "sr" | "mk" | "kk" | "mn"
            | "tg" | "tt" | "ba" | "el"
    )
}

/// Groups speech regions into recognition chunks of at most `max_ms`, joining regions
/// separated by short pauses, and pads each chunk so words at the edges aren't clipped.
/// Returns (start, end) in ms on the original timeline.
pub fn plan_chunks(speech: &[(i64, i64)], max_ms: i64, join_gap_ms: i64, pad_ms: i64, total_ms: i64) -> Vec<(i64, i64)> {
    let mut joined: Vec<(i64, i64)> = Vec::new();
    for &(s, e) in speech {
        match joined.last_mut() {
            Some(last) if s - last.1 < join_gap_ms && e - last.0 <= max_ms => last.1 = e,
            _ => joined.push((s, e)),
        }
    }
    // A long stretch without pauses (a newsreader, a song) is cut into equal parts, so
    // no chunk is longer than the recognizer can take (Whisper hears 30 s, Sarvam 30 s).
    let mut chunks: Vec<(i64, i64)> = Vec::with_capacity(joined.len());
    for (s, e) in joined {
        let parts = ((e - s) as f64 / max_ms.max(1) as f64).ceil().max(1.0) as i64;
        let step = (e - s) / parts;
        for k in 0..parts {
            chunks.push((s + k * step, if k + 1 == parts { e } else { s + (k + 1) * step }));
        }
    }
    let n = chunks.len();
    let mut padded = Vec::with_capacity(n);
    for i in 0..n {
        let (s, e) = chunks[i];
        // Pad into silence, but never past the midpoint of the gap to a neighbor.
        let lo = if i > 0 { (chunks[i - 1].1 + s) / 2 } else { 0 };
        let hi = if i + 1 < n { (e + chunks[i + 1].0) / 2 } else { total_ms.max(e) };
        padded.push(((s - pad_ms).max(lo), (e + pad_ms).min(hi)));
    }
    padded
}

/// Chunks for online recognition, which returns one time span per chunk: every chunk is
/// one stretch of speech between real pauses, so its text is pinned to when it was said.
///   - speech separated by a pause shorter than `join_gap_ms` stays together (a breath
///     inside a sentence), up to `max_ms`;
///   - a very short piece (under 1.2 s) joins its closer neighbour across a pause of up
///     to 1.5 s, because a recognizer needs some context;
///   - anything longer than `max_ms` is cut into equal parts.
pub fn plan_pause_chunks(speech: &[(i64, i64)], max_ms: i64, join_gap_ms: i64, pad_ms: i64, total_ms: i64) -> Vec<(i64, i64)> {
    let mut pieces: Vec<(i64, i64)> = Vec::new();
    for &(s, e) in speech {
        match pieces.last_mut() {
            Some(last) if s - last.1 < join_gap_ms && e - last.0 <= max_ms => last.1 = e,
            _ => pieces.push((s, e)),
        }
    }
    // Short pieces join the closer neighbour.
    let mut i = 0;
    while i < pieces.len() {
        let (s, e) = pieces[i];
        if e - s >= 1_200 || pieces.len() == 1 {
            i += 1;
            continue;
        }
        let before = (i > 0).then(|| (s - pieces[i - 1].1, e - pieces[i - 1].0));
        let after = (i + 1 < pieces.len()).then(|| (pieces[i + 1].0 - e, pieces[i + 1].1 - s));
        let ok = |x: Option<(i64, i64)>| x.filter(|&(gap, len)| gap <= 1_500 && len <= max_ms);
        match (ok(before), ok(after)) {
            (Some((gb, _)), Some((ga, _))) if ga < gb => {
                pieces[i + 1].0 = s;
                pieces.remove(i);
            }
            (Some(_), _) => {
                pieces[i - 1].1 = e;
                pieces.remove(i);
            }
            (None, Some(_)) => {
                pieces[i + 1].0 = s;
                pieces.remove(i);
            }
            (None, None) => i += 1,
        }
    }
    plan_chunks(&pieces, max_ms, 0, pad_ms, total_ms)
}

/// Stretches of speech (at least `min_ms` long) that no recognized word overlaps.
/// Used to catch audio Whisper skipped, so no speech goes uncaptioned silently.
pub fn uncovered(speech: &[(i64, i64)], words: &[Word], slack_ms: i64, min_ms: i64) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = words.iter().map(|w| (w.start_ms - slack_ms, w.end_ms.max(w.start_ms + 1) + slack_ms)).collect();
    spans.sort_unstable();
    let mut gaps = Vec::new();
    for &(s, e) in speech {
        let mut cursor = s;
        for &(a, b) in spans.iter().filter(|(a, b)| *b > s && *a < e) {
            if a > cursor && a - cursor >= min_ms {
                gaps.push((cursor, a.min(e)));
            }
            cursor = cursor.max(b);
        }
        if e > cursor && e - cursor >= min_ms {
            gaps.push((cursor, e));
        }
    }
    gaps
}

/// Total speech duration, for progress estimates and "no speech" decisions.
pub fn speech_ms(speech: &[(i64, i64)]) -> i64 {
    speech.iter().map(|(s, e)| e - s).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_output() {
        assert_eq!(parse_line("Speech segment 0: start = 67.00, end = 313.00"), Some((670, 3130)));
        assert_eq!(parse_line("  Speech segment 12: start = 1283.50, end = 1440.00"), Some((12835, 14400)));
        assert_eq!(parse_line("Detected 9 speech segments:"), None);
        assert_eq!(parse_line("Speech segment 1: start = 5.00, end = 5.00"), None);
    }

    #[test]
    fn drops_words_outside_speech() {
        let w = |s, e, t: &str| Word { confidence: 1.0, start_ms: s, end_ms: e, text: t.into() };
        let words = vec![w(100, 400, " hello"), w(5_000, 5_600, " invented"), w(9_000, 9_300, " world")];
        let kept = keep_speech(words, &[(0, 1_000), (8_800, 9_500)], 300);
        let texts: Vec<_> = kept.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, [" hello", " world"]);
    }

    #[test]
    fn chunks_join_short_pauses_and_respect_limits() {
        let speech = [(500, 3_000), (3_400, 6_000), (6_300, 14_000), (20_000, 21_000)];
        let chunks = plan_chunks(&speech, 12_000, 1_500, 250, 30_000);
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert_eq!(chunks[0], (250, 6_150));
        assert!(chunks[1].0 >= 6_150 && chunks[1].0 <= 6_300);
        assert!(chunks.windows(2).all(|w| w[0].1 <= w[1].0), "no overlap: {chunks:?}");
        assert_eq!(chunks[2].1, 21_250);
    }

    #[test]
    fn script_aware_chunk_limits() {
        assert_eq!(chunk_limit_ms(Some("en")), 24_000);
        assert_eq!(chunk_limit_ms(Some("bn")), 12_000);
        assert_eq!(chunk_limit_ms(Some("ar")), 12_000);
        assert_eq!(chunk_limit_ms(None), 12_000);
    }

    #[test]
    fn finds_speech_without_words() {
        let w = |s, e| Word { confidence: 1.0, start_ms: s, end_ms: e, text: " x".into() };
        let words = vec![w(1_000, 1_500), w(1_600, 2_000), w(9_000, 9_400)];
        let gaps = uncovered(&[(900, 6_000), (8_800, 9_500)], &words, 200, 1_500);
        assert_eq!(gaps, vec![(2_200, 6_000)]);
        assert!(uncovered(&[(900, 2_100)], &words, 200, 1_500).is_empty());
    }

    #[test]
    fn no_segments_means_no_filtering() {
        let words = vec![Word { confidence: 1.0, start_ms: 0, end_ms: 10, text: " a".into() }];
        assert_eq!(keep_speech(words.clone(), &[], 0), words);
    }

    #[test]
    fn long_stretches_without_pauses_are_cut_to_fit() {
        let chunks = plan_chunks(&[(1_000, 71_000), (80_000, 85_000)], 25_000, 1_500, 250, 90_000);
        assert_eq!(chunks.len(), 4, "{chunks:?}");
        assert!(chunks.iter().all(|(a, b)| b - a <= 25_000 + 500), "{chunks:?}");
        // Parts of one stretch touch without overlapping.
        assert!(chunks.windows(2).all(|w| w[0].1 <= w[1].0), "{chunks:?}");
    }

    #[test]
    fn online_chunks_never_run_across_a_real_pause() {
        // Another voice 0-5.2 s, a 0.9 s pause, then the narration; a breath at 8 s.
        let speech = [(0, 5_200), (6_100, 8_000), (8_250, 10_500), (14_000, 14_400), (15_000, 19_000)];
        let chunks = plan_pause_chunks(&speech, 12_000, 600, 200, 20_000);
        // 0-5.2 alone; 6.1-10.5 together (0.25 s breath); the 0.4 s word joins what follows.
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert!(chunks[0].1 <= 5_650 && chunks[1].0 >= 5_650, "{chunks:?}");
        assert!(chunks[1].0 <= 6_100 && chunks[1].1 >= 10_500, "{chunks:?}");
        assert!(chunks[2].0 <= 14_000 && chunks[2].1 >= 19_000, "{chunks:?}");
        assert!(chunks.windows(2).all(|w| w[0].1 <= w[1].0), "{chunks:?}");
    }

    #[test]
    fn online_chunks_stay_under_the_limit() {
        let speech = [(0, 9_500), (9_700, 19_000), (19_100, 28_000)];
        let chunks = plan_pause_chunks(&speech, 12_000, 600, 200, 30_000);
        assert!(chunks.iter().all(|(a, b)| b - a <= 12_400), "{chunks:?}");
        assert_eq!(chunks.len(), 3, "{chunks:?}");
    }
}
