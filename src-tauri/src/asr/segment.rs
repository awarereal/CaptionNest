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
    rebalance_tails(&mut groups, opts);

    let mut cues: Vec<Cue> = groups
        .iter()
        .map(|g| {
            let text = wrap_text(&join(g), opts.max_line_chars);
            let mut cue = Cue::new(g[0].start_ms, g[g.len() - 1].end_ms, text);
            cue.check = uncertain_words(g);
            cue
        })
        .collect();
    fix_timing(&mut cues, opts, media_end_ms);
    cues
}

/// When a long sentence spills over, the greedy split can leave a lonely last word
/// ("হয়।") as its own caption. Share the words of the two captions evenly instead.
fn rebalance_tails(groups: &mut [Vec<&Word>], opts: &Options) {
    for i in 1..groups.len() {
        let (head, tail) = groups.split_at_mut(i);
        let (prev, last) = (&mut head[i - 1], &mut tail[0]);
        let last_text = join(last);
        let prev_text = join(prev);
        let same_sentence = !ends_with_any(prev_text.trim_end(), SENTENCE_END);
        let close = last.first().zip(prev.last()).is_some_and(|(a, b)| a.start_ms - b.end_ms < 400);
        if !(same_sentence && close && graphemes(&last_text) < 14 && last.len() <= 2) {
            continue;
        }
        let all: Vec<&Word> = prev.iter().chain(last.iter()).copied().collect();
        let total: usize = all.iter().map(|w| graphemes(w.text.trim()) + 1).sum();
        let limit = line_limit(&join(&all), opts.max_line_chars) * opts.max_lines;
        // Split where the left part first reaches half the text, keeping both within limits.
        let mut acc = 0;
        let mut cut = all.len() - last.len();
        for (k, w) in all.iter().enumerate() {
            acc += graphemes(w.text.trim()) + 1;
            if acc * 2 >= total {
                cut = (k + 1).min(all.len() - 1).max(1);
                break;
            }
        }
        let (a, b) = all.split_at(cut);
        if graphemes(&join(a)) <= limit && graphemes(&join(b)) <= limit {
            *prev = a.to_vec();
            *last = b.to_vec();
        }
    }
}

/// Below this confidence a word is listed for the user to check.
pub const CHECK_BELOW: f32 = 0.5;

fn uncertain_words(words: &[&Word]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words.iter().filter(|w| w.confidence < CHECK_BELOW) {
        let t: String = w.text.trim().trim_matches(|c: char| !c.is_alphanumeric() && !('\u{0980}'..='\u{09FF}').contains(&c)).to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
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
    let words = drop_instant_duplicates(words);
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    for w in &words {
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
                prev.confidence = prev.confidence.min(w.confidence.max(CHECK_BELOW));
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

/// Whisper sometimes emits a run of words that all share one tiny time span, followed by
/// the same words again with real timing (seen with VAD and at window boundaries).
/// Such a run is dropped when the words right after it start at the same moment.
fn drop_instant_duplicates(words: &[Word]) -> Vec<Word> {
    let mut out = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let (s, e) = (words[i].start_ms, words[i].end_ms);
        let mut j = i;
        while j + 1 < words.len() && words[j + 1].start_ms == s && words[j + 1].end_ms == e {
            j += 1;
        }
        let tiny = e - s <= 150;
        let followed = words.get(j + 1).is_some_and(|n| (n.start_ms - s).abs() <= 30);
        let repeats = words.get(j + 1).is_some_and(|n| n.text.trim() == words[i].text.trim());
        if tiny && followed && (j > i || repeats) {
            i = j + 1;
            continue;
        }
        out.push(words[i].clone());
        i += 1;
    }
    out
}

/// Builds cues from sentence-level segments (used for translations, whose word timing
/// doesn't follow the audio). Long segments are split into readable cues at punctuation
/// or word boundaries, with time shared out in proportion to text length.
pub fn from_segments(segments: &[Word], opts: &Options, media_end_ms: Option<i64>) -> Vec<Cue> {
    let mut segments = drop_instant_duplicates(segments);
    // A decoding loop repeats the same sentence segment after segment: keep one,
    // spanning the whole stretch.
    let key = |t: &str| t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    segments.dedup_by(|b, a| {
        if !key(&a.text).is_empty() && key(&a.text) == key(&b.text) {
            a.end_ms = a.end_ms.max(b.end_ms);
            true
        } else {
            false
        }
    });
    let mut cues = Vec::new();
    for seg in &segments {
        let text = seg.text.trim();
        if text.is_empty() || is_noise(text) {
            continue;
        }
        let limit = line_limit(text, opts.max_line_chars) * opts.max_lines;
        let chunks = split_text(text, limit);
        let total: usize = chunks.iter().map(|c| graphemes(c).max(1)).sum();
        let span = (seg.end_ms - seg.start_ms).max(1);
        let mut at = seg.start_ms;
        for (k, chunk) in chunks.iter().enumerate() {
            let end = if k + 1 == chunks.len() { seg.end_ms } else { at + span * graphemes(chunk).max(1) as i64 / total as i64 };
            cues.push(Cue::new(at, end.max(at + 1), wrap_text(chunk, opts.max_line_chars)));
            at = end;
        }
    }
    fix_timing(&mut cues, opts, media_end_ms);
    cues
}

/// Splits text into pieces of at most `limit` graphemes, preferring to break after
/// sentence punctuation, then clause punctuation, then at spaces.
fn split_text(text: &str, limit: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = text.trim().to_string();
    while graphemes(&rest) > limit {
        let idx: Vec<(usize, char)> = rest.char_indices().collect();
        let cut_at_grapheme = |n: usize| rest.grapheme_indices(true).nth(n).map(|(i, _)| i).unwrap_or(rest.len());
        let hard = cut_at_grapheme(limit);
        let min = cut_at_grapheme(limit / 3);
        let pick = |set: &[char]| {
            idx.iter().rev().find(|(i, c)| *i < hard && *i >= min && set.contains(c)).map(|(i, c)| i + c.len_utf8())
        };
        let cut = pick(SENTENCE_END)
            .or_else(|| pick(CLAUSE_END))
            .or_else(|| idx.iter().rev().find(|(i, c)| *i <= hard && *i >= min && *c == ' ').map(|(i, _)| *i))
            .unwrap_or(hard);
        let (a, b) = rest.split_at(cut);
        pieces.push(a.trim().to_string());
        rest = b.trim().to_string();
    }
    if !rest.is_empty() {
        pieces.push(rest);
    }
    pieces
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
    space_after_sentence_marks(s.trim())
}

/// Whisper sometimes glues the next word to a sentence mark ("হের।এমএস").
fn space_after_sentence_marks(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if matches!(c, '।' | '॥' | '؟' | '?' | '!') {
            if let Some(n) = chars.peek() {
                if n.is_alphanumeric() {
                    out.push(' ');
                }
            }
        }
    }
    out
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
        spec.iter().map(|(s, e, t)| Word { confidence: 1.0, start_ms: *s, end_ms: *e, text: format!(" {t}") }).collect()
    }

    /// `n` copies of the same word.
    fn sentence(start: i64, n: usize, word: &str, step: i64) -> Vec<Word> {
        (0..n).map(|i| Word { confidence: 1.0, start_ms: start + i as i64 * step, end_ms: start + (i as i64 + 1) * step - 20, text: format!(" {word}") }).collect()
    }

    /// `n` different words ("word0", "word1", ...).
    fn varied(start: i64, n: usize, step: i64) -> Vec<Word> {
        (0..n).map(|i| Word { confidence: 1.0, start_ms: start + i as i64 * step, end_ms: start + (i as i64 + 1) * step - 20, text: format!(" word{i}") }).collect()
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
            w.push(Word { confidence: 1.0, start_ms: 1800, end_ms: 1800, text: " words".into() });
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
    fn drops_instant_duplicate_runs_from_vad() {
        let w = |s, e, t: &str| Word { confidence: 1.0, start_ms: s, end_ms: e, text: format!(" {t}") };
        // Real whisper.cpp output shape: three words sharing 6650-6750, then the real words.
        let words = vec![
            w(6250, 6650, "হয়।"),
            w(6650, 6750, "সক্রান্ত্র"), w(6650, 6750, "বিস্লেশন"), w(6650, 6750, "করে"),
            w(6650, 11160, "সক্রান্ত্র"), w(11160, 11940, "বিস্লেশন"), w(11940, 12510, "করে"),
        ];
        let cues = segment(&words, &Options::default(), None);
        let all: String = cues.iter().map(|c| c.text.replace('\n', " ")).collect::<Vec<_>>().join(" ");
        assert_eq!(all.matches("সক্রান্ত্র").count(), 1, "{all}");
    }

    #[test]
    fn translation_segments_split_into_readable_cues() {
        let seg = Word {
            confidence: 1.0,
            start_ms: 0,
            end_ms: 10_000,
            text: " It was supposed to be cut down on Tuesday, but it was saved after an emergency court ruling. The results will be posted online.".into(),
        };
        let cues = from_segments(&[seg], &Options::default(), None);
        assert!(cues.len() >= 2, "{cues:?}");
        assert!(cues.windows(2).all(|p| p[0].end_ms <= p[1].start_ms));
        assert_eq!(cues.first().unwrap().start_ms, 0);
        for c in &cues {
            assert!(c.text.lines().count() <= 2 && c.text.lines().all(|l| graphemes(l) <= 42), "{c:?}");
        }
        let joined: String = cues.iter().map(|c| c.text.replace('\n', " ")).collect::<Vec<_>>().join(" ");
        assert!(joined.starts_with("It was supposed") && joined.ends_with("posted online."));
    }

    #[test]
    fn repeated_translation_segments_collapse() {
        let seg = |s, e| Word { confidence: 1.0, start_ms: s, end_ms: e, text: " The first thing is to focus on public websites.".into() };
        let cues = from_segments(&[seg(12_000, 13_000), seg(13_000, 14_000), seg(14_000, 15_000)], &Options::default(), None);
        assert_eq!(cues.len(), 1, "{cues:?}");
        assert_eq!((cues[0].start_ms, cues[0].end_ms >= 15_000), (12_000, true));
    }

    #[test]
    fn low_confidence_words_are_listed_for_checking() {
        let mut w = words(&[(0, 300, "আমি"), (300, 700, "বাংলাজ"), (700, 1100, "কথা।")]);
        w[1].confidence = 0.31;
        let cues = segment(&w, &Options::default(), None);
        assert_eq!(cues[0].check, vec!["বাংলাজ".to_string()]);
    }

    #[test]
    fn sentence_tail_is_not_left_alone() {
        // A long Bangla sentence whose last word would otherwise be its own caption.
        let text = ["এটিকে", "মঙ্গলবার", "কেটে", "ফেলার", "কথা", "ছিল,", "তবে", "আদালতে", "জরুরি", "রায়", "আসার", "পরে", "এটি", "বাঁচানো", "হয়।"];
        let w: Vec<Word> = text.iter().enumerate().map(|(i, t)| Word { confidence: 1.0, start_ms: i as i64 * 450, end_ms: i as i64 * 450 + 400, text: format!(" {t}") }).collect();
        // 6.7 s of speech: the 6 s caption limit forces a split near the end.
        let opts = Options::default();
        let cues = segment(&w, &opts, None);
        assert!(cues.len() >= 2, "{cues:?}");
        assert!(cues.iter().all(|c| graphemes(&c.text) >= 14), "{cues:?}");
        let joined: String = cues.iter().map(|c| c.text.replace('\n', " ")).collect::<Vec<_>>().join(" ");
        assert!(joined.ends_with("বাঁচানো হয়।"));
    }

    #[test]
    fn adds_space_after_danda() {
        assert_eq!(space_after_sentence_marks("হের।এমএস পরিচিত"), "হের। এমএস পরিচিত");
        assert_eq!(space_after_sentence_marks("Done. 3.5 ok!Yes"), "Done. 3.5 ok! Yes");
    }

    #[test]
    fn attaches_standalone_punctuation() {
        let mut w = words(&[(0, 400, "Hello")]);
        w.push(Word { confidence: 1.0, start_ms: 400, end_ms: 450, text: "?".into() });
        let cues = segment(&w, &Options::default(), None);
        assert_eq!(cues[0].text, "Hello?");
    }
}
