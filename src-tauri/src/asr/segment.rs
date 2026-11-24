//! Turns word timings into readable caption cues.
//!
//! Rules (common subtitle practice):
//! - at most 2 lines of ~42 characters (about 18 for Chinese/Japanese/Korean)
//! - a cue lasts at most ~6 s and at least ~0.8 s when there is room
//! - prefer to end a cue at the end of a sentence, then at a comma, then at a pause
//! - long cues are split into two balanced lines at a word boundary
//! - non-speech markers ("[BLANK_AUDIO]", "(music)", "♪") and runaway repetition are dropped

use unicode_segmentation::UnicodeSegmentation;

use super::Word;
use crate::captions::Cue;

#[derive(Debug, Clone)]
pub struct Options {
    pub max_line_chars: usize,
    pub max_lines: usize,
    pub max_duration_ms: i64,
    pub min_duration_ms: i64,
    /// A pause longer than this always starts a new cue.
    pub max_gap_ms: i64,
    /// Extra time a cue stays on screen after the last word, if the next cue allows.
    pub linger_ms: i64,
}

impl Default for Options {
    fn default() -> Self {
        Self { max_line_chars: 42, max_lines: 2, max_duration_ms: 6_000, min_duration_ms: 800, max_gap_ms: 800, linger_ms: 250 }
    }
}

const SENTENCE_END: &[char] = &['.', '?', '!', '।', '॥', '؟', '۔', '。', '？', '！', '…'];
const CLAUSE_END: &[char] = &[',', ';', ':', '،', '؛', '、', '，', '；'];

pub fn segment(words: &[Word], opts: &Options, media_end_ms: Option<i64>) -> Vec<Cue> {
    let words = clean(words);
    let mut groups: Vec<Vec<&Word>> = Vec::new();
    let mut cur: Vec<&Word> = Vec::new();

    for w in &words {
        if let (Some(first), Some(prev)) = (cur.first(), cur.last()) {
            let text_now = join(&cur);
            let limit = line_limit(&text_now, opts.max_line_chars) * opts.max_lines;
            let len_now = graphemes(&text_now);
            let len_with = graphemes(&format!("{text_now}{}", w.text).trim().to_string());
            let prev_text = prev.text.trim_end();
            let gap = w.start_ms - prev.end_ms;
            let break_here = gap > opts.max_gap_ms
                || w.end_ms - first.start_ms > opts.max_duration_ms
                || len_with > limit
                || (ends_with_any(prev_text, SENTENCE_END) && len_now >= 12)
                || (ends_with_any(prev_text, CLAUSE_END) && len_now >= limit / 2);
            if break_here {
                groups.push(std::mem::take(&mut cur));
            }
        }
        cur.push(w);
    }
    if !cur.is_empty() {
        groups.push(cur);
    }

    let mut cues: Vec<Cue> = groups
        .iter()
        .map(|g| {
            let text = wrap_text(&join(g), opts.max_line_chars);
            Cue::new(g[0].start_ms, g[g.len() - 1].end_ms, text)
        })
        .collect();
    fix_timing(&mut cues, opts, media_end_ms);
    cues
}

/// Applies minimum duration and linger without creating overlaps.
fn fix_timing(cues: &mut [Cue], opts: &Options, media_end_ms: Option<i64>) {
    for i in 0..cues.len() {
        let next_start = cues.get(i + 1).map(|c| c.start_ms).or(media_end_ms).unwrap_or(i64::MAX);
        let c = &mut cues[i];
        let wanted = (c.end_ms + opts.linger_ms).max(c.start_ms + opts.min_duration_ms);
        c.end_ms = wanted.min(next_start).max(c.end_ms.min(next_start));
        if c.end_ms <= c.start_ms {
            c.end_ms = c.start_ms + 1;
        }
    }
}

/// Removes non-speech markers and runaway repetition, merges punctuation-only tokens.
fn clean(words: &[Word]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    for w in words {
        let t = w.text.trim();
        if t.is_empty() || is_noise(t) {
            continue;
        }
        if let Some(prev) = out.last_mut() {
            // A zero-length copy of the previous word is a decoding loop, not speech.
            if w.end_ms <= w.start_ms && prev.text.trim() == t {
                continue;
            }
            // Punctuation that arrived as its own token belongs to the previous word.
            if t.chars().all(|c| SENTENCE_END.contains(&c) || CLAUSE_END.contains(&c)) {
                prev.text.push_str(t);
                prev.end_ms = prev.end_ms.max(w.end_ms);
                continue;
            }
        }
        out.push(w.clone());
    }
    // A word repeated more than 3 times in a row is a decoding loop: keep 3 at most.
    let mut result: Vec<Word> = Vec::with_capacity(out.len());
    for w in out {
        let n = result.iter().rev().take_while(|p| p.text.trim() == w.text.trim()).count();
        if n >= 3 {
            continue;
        }
        result.push(w);
    }
    result
}

fn is_noise(t: &str) -> bool {
    let bracketed = |o: char, c: char| t.starts_with(o) && t.ends_with(c);
    bracketed('[', ']') || bracketed('(', ')') || bracketed('*', '*') || t.chars().all(|c| matches!(c, '♪' | '♫' | '♬' | '-' | ' '))
}

fn join(words: &[&Word]) -> String {
    let mut s = String::new();
    for w in words {
        s.push_str(&w.text);
    }
    s.trim().to_string()
}

fn ends_with_any(s: &str, set: &[char]) -> bool {
    s.chars().last().is_some_and(|c| set.contains(&c))
}

pub fn graphemes(s: &str) -> usize {
    s.graphemes(true).count()
}

/// CJK text is roughly twice as wide per character, so lines hold about half as many.
fn line_limit(text: &str, latin_limit: usize) -> usize {
    let (mut cjk, mut total) = (0usize, 0usize);
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF) {
            cjk += 1;
        }
    }
    if total > 0 && cjk * 2 > total { (latin_limit * 3 / 7).max(12) } else { latin_limit }
}

/// Splits text that is longer than one line into two balanced lines at a word boundary,
/// preferring a break right after punctuation. Text that already contains line breaks
/// is left as it is.
pub fn wrap_text(text: &str, max_line_chars: usize) -> String {
    let text = text.trim();
    let limit = line_limit(text, max_line_chars);
    if text.contains('\n') || graphemes(text) <= limit {
        return text.to_string();
    }
    let total = graphemes(text);
    let spaces: Vec<usize> = text.char_indices().filter(|(_, c)| *c == ' ').map(|(i, _)| i).collect();
    if spaces.is_empty() {
        // No spaces (e.g. Chinese/Japanese): split at the grapheme midpoint.
        let mid = total / 2;
        let idx = text.grapheme_indices(true).nth(mid).map(|(i, _)| i).unwrap_or(text.len());
        return format!("{}\n{}", &text[..idx], &text[idx..]);
    }
    let best = spaces
        .iter()
        .min_by_key(|&&i| {
            let left = graphemes(&text[..i]);
            let right = total - left - 1;
            let imbalance = left.abs_diff(right) as i64;
            let after_punct = text[..i].chars().last().is_some_and(|c| SENTENCE_END.contains(&c) || CLAUSE_END.contains(&c));
            let over = (left.max(right) as i64 - limit as i64).max(0) * 4;
            imbalance + over - if after_punct { 6 } else { 0 }
        })
        .copied()
        .unwrap();
    format!("{}\n{}", text[..best].trim_end(), text[best + 1..].trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(spec: &[(i64, i64, &str)]) -> Vec<Word> {
        spec.iter().map(|(s, e, t)| Word { start_ms: *s, end_ms: *e, text: format!(" {t}") }).collect()
    }

    /// `n` copies of the same word.
    fn sentence(start: i64, n: usize, word: &str, step: i64) -> Vec<Word> {
        (0..n).map(|i| Word { start_ms: start + i as i64 * step, end_ms: start + (i as i64 + 1) * step - 20, text: format!(" {word}") }).collect()
    }

    /// `n` different words ("word0", "word1", ...).
    fn varied(start: i64, n: usize, step: i64) -> Vec<Word> {
        (0..n).map(|i| Word { start_ms: start + i as i64 * step, end_ms: start + (i as i64 + 1) * step - 20, text: format!(" word{i}") }).collect()
    }

    #[test]
    fn short_sentence_is_one_cue_with_linger() {
        let cues = segment(&words(&[(0, 300, "Hello"), (300, 700, "there.")]), &Options::default(), None);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Hello there.");
        assert_eq!(cues[0].start_ms, 0);
        assert_eq!(cues[0].end_ms, 950);
    }

    #[test]
    fn breaks_at_sentence_end_and_pauses() {
        let w = words(&[
            (0, 400, "This"), (400, 800, "is"), (800, 1200, "the"), (1200, 1600, "first."),
            (1700, 2000, "Second"), (2000, 2400, "one"),
            (5000, 5400, "After"), (5400, 5800, "pause"),
        ]);
        let cues = segment(&w, &Options::default(), None);
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["This is the first.", "Second one", "After pause"]);
        assert!(cues.windows(2).all(|p| p[0].end_ms <= p[1].start_ms), "no overlaps");
    }

    #[test]
    fn long_speech_respects_length_and_duration_limits() {
        let w = varied(0, 60, 300);
        let opts = Options::default();
        let cues = segment(&w, &opts, None);
        assert!(cues.len() > 3);
        for c in &cues {
            let lines: Vec<&str> = c.text.lines().collect();
            assert!(lines.len() <= 2, "{:?}", c.text);
            assert!(lines.iter().all(|l| graphemes(l) <= 42), "{:?}", c.text);
            assert!(c.end_ms - c.start_ms <= opts.max_duration_ms + opts.linger_ms);
        }
    }

    #[test]
    fn bangla_fixture_becomes_readable_cues() {
        let t = crate::asr::whisper::parse_json(include_bytes!("../../tests/fixtures/whisper_bn_words.json")).unwrap();
        let cues = segment(&t.words, &Options::default(), Some(7_400));
        assert!(!cues.is_empty());
        let all: String = cues.iter().map(|c| c.text.replace('\n', " ")).collect::<Vec<_>>().join(" ");
        assert!(all.starts_with("আমার নাম"));
        assert!(all.ends_with("জাবো।"));
        for c in &cues {
            assert!(c.text.lines().count() <= 2);
            assert!(c.end_ms > c.start_ms && c.end_ms <= 7_400);
        }
        assert!(cues.windows(2).all(|p| p[0].end_ms <= p[1].start_ms));
    }

    #[test]
    fn drops_noise_markers_and_loops() {
        let mut w = words(&[(0, 500, "[BLANK_AUDIO]"), (500, 900, "(music)"), (900, 1000, "♪"), (1000, 1400, "Real"), (1400, 1800, "words")]);
        for _ in 0..10 {
            w.push(Word { start_ms: 1800, end_ms: 1800, text: " words".into() });
        }
        let cues = segment(&w, &Options::default(), None);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Real words");
        assert_eq!(cues[0].start_ms, 1000);
    }

    #[test]
    fn collapses_runaway_repetition() {
        let w = sentence(0, 12, "মার্", 400);
        let cues = segment(&w, &Options::default(), None);
        let n: usize = cues.iter().map(|c| c.text.split_whitespace().count()).sum();
        assert!(n <= 3, "{cues:?}");
    }

    #[test]
    fn wraps_into_balanced_lines() {
        let wrapped = wrap_text("The quick brown fox jumps over the lazy dog near the river", 42);
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(graphemes(lines[0]).abs_diff(graphemes(lines[1])) <= 8, "{wrapped}");
        assert_eq!(wrap_text("Short line", 42), "Short line");
        assert_eq!(wrap_text("Keep\nmine", 4), "Keep\nmine");
    }

    #[test]
    fn wraps_cjk_without_spaces() {
        let wrapped = wrap_text("今日はとても良い天気ですね散歩に行きましょう", 42);
        assert_eq!(wrapped.lines().count(), 2);
    }

    #[test]
    fn attaches_standalone_punctuation() {
        let mut w = words(&[(0, 400, "Hello")]);
        w.push(Word { start_ms: 400, end_ms: 450, text: "?".into() });
        let cues = segment(&w, &Options::default(), None);
        assert_eq!(cues[0].text, "Hello?");
    }
}
