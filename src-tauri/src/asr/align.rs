//! Word timing for online transcripts.
//!
//! Sarvam returns the text of each chunk (up to 25 s) with one time span for the whole
//! chunk, so on its own every caption inside a chunk would be placed by guesswork.
//! CaptionNest listens to the same audio with an offline model, which gives a time for
//! every word (its spelling is worse, its timing is real), and lines the two transcripts
//! up letter by letter: each online word takes the time of the offline letters it
//! matches. Words the offline model didn't hear are placed in the speech between their
//! neighbours.
//!
//! Without an offline model, words are spread over the stretches of real speech (from
//! the VAD) instead of evenly over the chunk, so pauses don't pull captions off.
//!
//! The text is never changed here: every word that comes in goes out, in order.

use super::Word;

/// Online phrases (text with a chunk-wide span) → the same words, each with a time.
pub fn place_words(phrases: &[Word], timed: Option<&[Word]>, speech: &[(i64, i64)]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    for p in phrases {
        let words = split_words(&collapse_repeats(&p.text));
        if words.is_empty() {
            continue;
        }
        let (lo, hi) = (p.start_ms, p.end_ms.max(p.start_ms + 1));
        let acoustic: Vec<&Word> = timed
            .map(|t| t.iter().filter(|w| w.end_ms > lo - 300 && w.start_ms < hi + 300 && !letters(&w.text).is_empty()).collect())
            .unwrap_or_default();
        let mut spans = align(&words, &acoustic, lo, hi, speech).unwrap_or_else(|| spread_over_speech(&words, lo, hi, speech));
        settle(&words, &mut spans, lo, hi);
        // Never before the previous chunk's last word.
        let floor = out.last().map_or(i64::MIN, |w| w.start_ms);
        out.extend(words.into_iter().zip(spans).map(|(text, (s, e))| {
            let s = s.max(floor);
            Word { start_ms: s, end_ms: e.max(s + 1), text: format!(" {text}"), confidence: 1.0 }
        }));
    }
    out
}

/// Letters per second below which a chunk's text can't fill the chunk: the voice is
/// only part of it (the rest is music, noise or people talking in the background).
/// Normal speech runs at 10-17 letters a second.
const SPARSE_PACE: i64 = 7;

/// Whether a phrase is too short for the speech time of its chunk, so spreading its
/// words over the chunk would leave captions lingering or early.
pub fn is_sparse(phrase: &Word, speech: &[(i64, i64)]) -> bool {
    let (lo, hi) = (phrase.start_ms, phrase.end_ms);
    let talk: i64 = speech.iter().map(|&(s, e)| (e.min(hi) - s.max(lo)).max(0)).sum();
    let talk = if speech.is_empty() { hi - lo } else { talk };
    let count = letters(&collapse_repeats(&phrase.text)).len() as i64;
    talk >= 3_000 && count > 0 && count * 1000 / talk < SPARSE_PACE
}

fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// A line or sentence repeated more than twice in a row is a recognizer loop (often on
/// music or crowd noise), not speech: keep two.
pub fn collapse_repeats(text: &str) -> String {
    let key = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    // Sentences: split after sentence marks and line breaks, keeping the marks.
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            if !cur.trim().is_empty() {
                parts.push(cur.trim().to_string());
            }
            cur.clear();
            continue;
        }
        cur.push(c);
        // A period ends a sentence only before a space ("২.৫০" stays whole).
        let period = c == '.' && chars.peek().is_none_or(|n| n.is_whitespace());
        if matches!(c, '।' | '?' | '!' | '॥') || period {
            parts.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    let mut kept: Vec<String> = Vec::new();
    for part in parts {
        let k = key(&part);
        let run = kept.iter().rev().take_while(|p| key(p) == k && !k.is_empty()).count();
        if run < 2 {
            kept.push(part);
        }
    }
    kept.join(" ")
}

/// Letters that count when matching: letters and vowel signs, not digits or punctuation.
pub(crate) fn letters(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| c.is_alphabetic() || (('\u{0980}'..='\u{09FF}').contains(c) && !('\u{09E6}'..='\u{09EF}').contains(c) && *c != '\u{09F3}'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// How long a word needs at the very least: about half of normal speaking pace.
fn min_ms(word: &str) -> i64 {
    (letters(word).len().max(1) as i64 * 35).clamp(80, 500)
}

/// How alike two words are, 0..1: shared letters in order over the longer word.
pub(crate) fn similarity(a: &[char], b: &[char]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut row = vec![0u16; b.len() + 1];
    for &ca in a {
        let mut diag = 0;
        for (j, &cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if ca == cb { diag + 1 } else { up.max(row[j]) };
            diag = up;
        }
    }
    f32::from(row[b.len()]) / a.len().max(b.len()) as f32
}

/// Times for `words` from the offline words that clearly are the same words, or None if
/// too few agree. Only sure matches count: the same word (or nearly) in the same order,
/// said at a believable pace. Everything else is placed between those anchors.
fn align(words: &[String], acoustic: &[&Word], lo: i64, hi: i64, speech: &[(i64, i64)]) -> Option<Vec<(i64, i64)>> {
    let ol: Vec<Vec<char>> = words.iter().map(|w| letters(w)).collect();
    let al: Vec<Vec<char>> = acoustic.iter().map(|w| letters(&w.text)).collect();
    let (n, m) = (ol.len(), al.len());
    if n == 0 || m == 0 {
        return None;
    }
    // Offline words whose own timing can't be trusted: no length at all, or part of a
    // stretch spoken implausibly slowly (Whisper smears words over audio it skipped).
    let usable: Vec<bool> = (0..m)
        .map(|j| {
            let (a, b) = (j.saturating_sub(1), (j + 1).min(m - 1));
            let count: usize = al[a..=b].iter().map(Vec::len).sum();
            let ms = (acoustic[b].end_ms - acoustic[a].start_ms).max(1);
            // Inside this chunk (its text can't be anywhere else) and at a real pace.
            let inside = acoustic[j].start_ms >= lo - 150 && acoustic[j].start_ms <= hi;
            inside && acoustic[j].end_ms > acoustic[j].start_ms && count as i64 * 1000 / ms >= 8
        })
        .collect();
    // Best in-order pairing by weight (how alike × how long).
    let weight = |i: usize, j: usize| -> f32 {
        if !usable[j] || ol[i].len() < 2 || al[j].len() < 2 {
            return 0.0;
        }
        let sim = similarity(&ol[i], &al[j]);
        if sim >= 0.6 { sim * ol[i].len().min(al[j].len()) as f32 } else { 0.0 }
    };
    let w = m + 1;
    let mut best = vec![0f32; (n + 1) * w];
    let mut pair = vec![0f32; n * m];
    for i in 0..n {
        for j in 0..m {
            let p = weight(i, j);
            pair[i * m + j] = p;
            let mut v = best[i * w + j + 1].max(best[(i + 1) * w + j]);
            if p > 0.0 {
                v = v.max(best[i * w + j] + p);
            }
            best[(i + 1) * w + j + 1] = v;
        }
    }
    let mut matched: Vec<Option<usize>> = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let here = best[i * w + j];
        let p = pair[(i - 1) * m + j - 1];
        if p > 0.0 && (here - (best[(i - 1) * w + j - 1] + p)).abs() < 1e-4 {
            matched[i - 1] = Some(j - 1);
            i -= 1;
            j -= 1;
        } else if (here - best[(i - 1) * w + j]).abs() < 1e-4 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    // An anchor is a long, nearly identical word, or part of a run of matches.
    let strong: Vec<bool> = (0..n)
        .map(|i| {
            let Some(j) = matched[i] else { return false };
            let alone = similarity(&ol[i], &al[j]) >= 0.75 && ol[i].len() >= 4;
            let prev = i > 0 && j > 0 && matched[i - 1] == Some(j - 1);
            let next = i + 1 < n && matched[i + 1] == Some(j + 1);
            alone || prev || next
        })
        .collect();
    let anchors = strong.iter().filter(|s| **s).count();
    if anchors < 2 && anchors * 5 < n {
        return None;
    }
    let spans: Vec<Option<(i64, i64)>> = (0..n)
        .map(|i| {
            let j = matched[i].filter(|_| strong[i])?;
            let s = acoustic[j].start_ms.clamp(lo, hi);
            let e = acoustic[j].end_ms.min(s + ol[i].len() as i64 * 120 + 150).clamp(s, hi);
            Some((s, e))
        })
        .collect();
    Some(fill_gaps(words, &spans, lo, hi, speech))
}

/// Words without a time of their own are placed in the speech between their timed
/// neighbours; times never run backwards.
fn fill_gaps(words: &[String], spans: &[Option<(i64, i64)>], lo: i64, hi: i64, speech: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let n = words.len();
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if let Some((s, e)) = spans[i] {
            let s = s.max(out.last().map_or(lo, |p| p.0));
            out.push((s, e.max(s)));
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && spans[j].is_none() {
            j += 1;
        }
        let from = out.last().map_or(lo, |p| p.1);
        let to = if j < n { spans[j].map_or(hi, |s| s.0) } else { hi };
        out.extend(spread_over_speech(&words[i..j], from, to.max(from), speech));
        i = j;
    }
    out
}

/// Gives every word at least a short time of its own and keeps the words in order
/// inside [lo, hi], borrowing from neighbours where words were squeezed together.
fn settle(words: &[String], spans: &mut [(i64, i64)], lo: i64, hi: i64) {
    let room = (hi - lo).max(1);
    let mut mins: Vec<i64> = words.iter().map(|w| min_ms(w)).collect();
    let need: i64 = mins.iter().sum();
    if need > room {
        for m in &mut mins {
            *m = (*m * room / need).max(1);
        }
    }
    // Forward: no overlap, minimum length.
    let mut at = lo;
    for (span, &m) in spans.iter_mut().zip(&mins) {
        span.0 = span.0.max(at);
        span.1 = span.1.max(span.0 + m);
        at = span.1;
    }
    // Backward: pull back whatever was pushed past the end.
    let mut limit = hi;
    for (span, &m) in spans.iter_mut().zip(&mins).rev() {
        if span.1 > limit {
            span.1 = limit;
            span.0 = span.0.min(span.1 - m).max(lo);
        }
        limit = span.0;
    }
}

/// Spreads words over the real speech inside [start, end], in proportion to length.
fn spread_over_speech(words: &[String], start: i64, end: i64, speech: &[(i64, i64)]) -> Vec<(i64, i64)> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut regions: Vec<(i64, i64)> = speech.iter().map(|&(s, e)| (s.max(start), e.min(end))).filter(|(s, e)| e > s).collect();
    if regions.is_empty() {
        regions.push((start, end.max(start)));
    }
    let talk: i64 = regions.iter().map(|(s, e)| e - s).sum();
    let weights: Vec<i64> = words.iter().map(|w| letters(w).len().max(1) as i64).collect();
    let total: i64 = weights.iter().sum::<i64>().max(1);
    // Where each region starts along the speech (0..talk).
    let mut offsets = Vec::with_capacity(regions.len());
    let mut sum = 0;
    for &(s, e) in &regions {
        offsets.push(sum);
        sum += e - s;
    }
    let region_of = |pos: i64| offsets.iter().rposition(|&o| o <= pos).unwrap_or(0);
    let time = |r: usize, pos: i64| regions[r].0 + (pos - offsets[r]).clamp(0, regions[r].1 - regions[r].0);
    let mut acc = 0;
    weights
        .iter()
        .map(|&w| {
            let pa = talk * acc / total;
            acc += w;
            let pb = talk * acc / total;
            let (ra, rb) = (region_of(pa), region_of((pb - 1).max(pa)));
            if ra == rb {
                return (time(ra, pa), time(rb, pb).max(time(ra, pa)));
            }
            // A word never straddles a pause: it goes where most of it falls.
            let in_first = offsets[ra] + (regions[ra].1 - regions[ra].0) - pa;
            let in_last = pb - offsets[rb];
            if in_last >= in_first {
                (regions[rb].0, time(rb, pb).max(regions[rb].0))
            } else {
                (time(ra, pa), regions[ra].1)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: i64, e: i64, t: &str) -> Word {
        Word { start_ms: s, end_ms: e, text: format!(" {t}"), confidence: 1.0 }
    }

    fn text(words: &[Word]) -> String {
        words.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn online_words_take_the_offline_timing() {
        // Sarvam: one span for the chunk. Offline: misspelled but timed words.
        let phrase = w(0, 22_000, "আসসালামু আলাইকুম। এটিএন বাংলা সংবাদে স্বাগত।");
        let offline = vec![w(9_100, 9_500, "আসলাম"), w(9_500, 10_100, "ওয়ালাইকুম"), w(10_300, 10_700, "এটিএন"), w(10_700, 11_050, "বাংলা"), w(11_050, 11_500, "সংবাদে"), w(11_550, 11_900, "সাগত")];
        let placed = place_words(&[phrase], Some(&offline), &[]);
        assert_eq!(placed.len(), 6);
        assert!((placed[0].start_ms - 9_100).abs() < 300, "{placed:?}");
        assert!((placed[2].start_ms - 10_300).abs() < 200, "{placed:?}");
        assert!((placed[5].end_ms - 11_900).abs() < 300, "{placed:?}");
        assert!(placed.windows(2).all(|p| p[0].start_ms <= p[1].start_ms));
    }

    #[test]
    fn unmatched_words_sit_between_their_neighbours() {
        let phrase = w(0, 10_000, "আমরা ৩০০০ টাকা দিলাম");
        let offline = vec![w(1_000, 1_600, "আমরা"), w(1_600, 2_800, "তিন হাজার"), w(2_800, 3_300, "টাকা"), w(3_300, 4_000, "দিলাম")];
        let placed = place_words(&[phrase], Some(&offline), &[]);
        assert!(placed[1].start_ms >= placed[0].end_ms - 1 && placed[1].end_ms <= placed[2].start_ms + 1, "{placed:?}");
        assert!(placed[1].end_ms - placed[1].start_ms >= 80, "{placed:?}");
    }

    #[test]
    fn no_word_is_ever_lost_or_changed() {
        // The offline pass missed the middle completely and left no room for it.
        let said = "রিজার্ভ করে নিছি এবং ৩০০০ টাকা-এ ৩-টা সিএনজি এবং ১৩ জন। এই। কি করছিস? এই। কিরে, বুঝলাম না।";
        let offline = vec![w(17_400, 18_000, "রিজাভ"), w(18_000, 18_300, "করে"), w(18_300, 18_800, "নিছি"), w(18_800, 19_300, "সিএনজি"), w(19_300, 19_600, "এবং")];
        let placed = place_words(&[w(17_000, 26_000, said)], Some(&offline), &[(17_300, 25_800)]);
        assert_eq!(text(&placed), said);
        assert!(placed.iter().all(|p| p.end_ms - p.start_ms >= 30), "{placed:?}");
        assert!(placed.windows(2).all(|p| p[0].end_ms <= p[1].start_ms), "{placed:?}");
        assert!(placed.iter().all(|p| p.start_ms >= 17_000 && p.end_ms <= 26_000), "{placed:?}");
        // With nothing matching at all, the words still all come out.
        let noise = vec![w(17_400, 18_000, "xyz")];
        assert_eq!(text(&place_words(&[w(17_000, 26_000, said)], Some(&noise), &[])), said);
    }

    #[test]
    fn a_short_sentence_in_a_long_noisy_chunk_goes_where_it_was_heard() {
        // Chunk 27.9-36.2 s is "speech" to the VAD (music, chatter), but the sentence
        // takes only its first two seconds.
        let phrase = w(27_890, 36_200, "কমেন্টটা সুন্দর থাকায় মারামারি লাগায় দিই।");
        let speech = [(27_890, 36_200)];
        assert!(is_sparse(&phrase, &speech));
        let offline = vec![w(27_890, 28_250, "ওমাইটা"), w(28_250, 28_670, "সুন্ধুর"), w(28_670, 28_910, "থাকে"), w(28_910, 29_390, "মারামারি"), w(29_390, 29_690, "লাগায়"), w(29_690, 29_890, "দি।")];
        let placed = place_words(&[phrase.clone()], Some(&offline), &speech);
        assert!(placed.last().unwrap().end_ms <= 30_600, "{placed:?}");
        // Without the offline pass the sentence is stretched over the whole chunk.
        let stretched = place_words(&[phrase], None, &speech);
        assert!(stretched.last().unwrap().end_ms > 35_000);
        // A normally paced chunk isn't sparse.
        assert!(!is_sparse(&w(0, 4_000, "আজকে আমরা যাব সীতাকুণ্ড ইকো পার্ক, এরপর যাব বাঁশবাড়িয়া সমুদ্র সৈকত।"), &[(0, 4_000)]));
    }

    #[test]
    fn offline_words_outside_the_chunk_are_not_anchors() {
        // Whisper smeared the sentence's end past the chunk: those times are ignored.
        let phrase = w(74_000, 80_000, "এখানে দোকানগুলো আপনাদের জুতা রাখতে পারবেন");
        let offline = vec![w(74_050, 74_350, "এখানে"), w(74_350, 74_890, "দোকানগুলো"), w(74_890, 75_310, "আপনাদের"), w(81_000, 81_600, "জুতা"), w(81_600, 82_200, "রাখতে"), w(82_200, 83_000, "পারবেন")];
        let offline_in_range: Vec<Word> = offline.clone();
        let placed = place_words(&[phrase], Some(&offline_in_range), &[(74_000, 80_000)]);
        assert!(placed.iter().all(|p| p.end_ms <= 80_000), "{placed:?}");
        assert!((placed[0].start_ms - 74_050).abs() < 100 && placed[3].start_ms >= 75_300, "{placed:?}");
    }

    #[test]
    fn repeated_loops_are_cut_to_two() {
        assert_eq!(collapse_repeats("ভাই কি করছিস?\nভাই।\nভাই।\nভাই।\nভাই। ভাই। ভাই।\nচলো।"), "ভাই কি করছিস? ভাই। ভাই। চলো।");
        assert_eq!(collapse_repeats("দাম ২.৫০ টাকা। এক জোড়া, এক জোড়া।"), "দাম ২.৫০ টাকা। এক জোড়া, এক জোড়া।");
    }

    #[test]
    fn without_offline_words_follow_the_speech_not_the_pauses() {
        // Speech 0-2 s and 8-10 s; the 6 s pause in the middle gets no words.
        let phrase = w(0, 10_000, "এক দুই তিন চার");
        let placed = place_words(&[phrase], None, &[(0, 2_000), (8_000, 10_000)]);
        assert!(placed.iter().all(|p| p.start_ms < 2_000 || p.start_ms >= 8_000), "{placed:?}");
        assert!(placed[2].start_ms >= 8_000, "{placed:?}");
    }
}
