//! Auto-sync: moves existing captions to where their words are spoken.
//!
//! An offline model listens to the video and gives a time for every word it hears.
//! The captions' words are matched to the heard words, in order, and every caption
//! takes the time of its own words. Only times change: text, order and line breaks
//! are never touched.
//!
//! How a caption is placed:
//!   1. Rare words that appear once in the captions and once in the audio show the
//!      overall trend: a steady offset, a drift that grows, or jumps.
//!   2. Around that trend, caption words are paired with heard words that are the same
//!      word (or nearly: the offline model misspells), keeping the order.
//!   3. The pairs say which stretch of speech (between two pauses) each caption word
//!      belongs to. The model's own word times aren't used: measured against exactly
//!      timed speech they wander by a second or more inside a stretch. Pauses, found
//!      from the sound itself, are exact. Inside a stretch the words are laid out by
//!      length, like a ruler: with the caption text when it covers what was said, with
//!      the heard text when more was said than the captions hold.
//!   4. A caption that couldn't be heard is never matched by guesswork: it takes the
//!      place the layout or its neighbours give it and is marked for review, or stays
//!      where it was.

use std::collections::HashMap;

use serde::Serialize;

use super::align::{letters, similarity};
use super::Word;
use crate::captions::Cue;

/// How far from the expected time a caption word may be heard.
const BAND_MS: i64 = 15_000;
/// Time a caption stays after its last word, if the next caption allows.
const LINGER_MS: i64 = 150;
/// A shift smaller than this isn't a correction, it's noise.
const SAME_MS: i64 = 80;
/// A caption that starts mid-speech is placed by length, good to about this much. Within
/// it of its neighbours' shift, it is moving with them.
const AGREE_MS: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Its words were found in the audio; the times come from there.
    Matched,
    /// Not found itself; moved by the same amount as the captions around it. Review.
    Estimated,
    /// Not found, and the captions around it don't agree: left as it was. Review.
    Unmatched,
    /// Outside the section that was synced: untouched.
    Outside,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Synced {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub old_start_ms: i64,
    pub old_end_ms: i64,
    pub status: Status,
    /// Share of the caption's letters found in the audio, 0..1.
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Nothing needed moving.
    InSync,
    /// Everything was early or late by about the same amount.
    Offset,
    /// The error grew (or shrank) steadily through the video.
    Drift,
    /// Different captions were off by different amounts.
    Mixed,
    /// The captions' words weren't heard in this audio.
    NoMatch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub kind: Kind,
    pub matched: usize,
    pub estimated: usize,
    pub unmatched: usize,
    /// Captions whose times are different now.
    pub changed: usize,
    pub median_shift_ms: i64,
    /// Shift near the start and near the end of the synced captions.
    pub first_shift_ms: i64,
    pub last_shift_ms: i64,
    pub max_shift_ms: i64,
}

struct CapWord {
    /// Which caption it belongs to (position in time order).
    cue: usize,
    letters: Vec<char>,
    skeleton: Vec<char>,
    latin: bool,
    /// Where the word is now, by its place in its caption.
    at: i64,
}

struct Heard {
    letters: Vec<char>,
    skeleton: Vec<char>,
    latin: bool,
    start: i64,
    end: i64,
    /// The stretch of speech it was heard in (index into the speech regions).
    region: usize,
}

impl Heard {
    /// End of the word, cut short when the model smeared it over audio it skipped.
    fn end(&self) -> i64 {
        self.end.min(self.start + self.letters.len() as i64 * 120 + 150).max(self.start + 1)
    }
}

fn is_latin(l: &[char]) -> bool {
    !l.is_empty() && l.iter().all(char::is_ascii)
}

/// The consonant sounds of a word, written the same for Bengali and Latin script, so
/// "expensive" and "এক্সপেন্সিভ" can be recognized as one word in mixed speech.
pub fn skeleton(word: &str) -> Vec<char> {
    let chars: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let mut step = 1;
        let sound: &str = match c {
            'ক' | 'খ' => "k",
            'গ' | 'ঘ' => "g",
            'ঙ' | 'ঞ' | 'ণ' | 'ন' | 'ং' => "n",
            'চ' | 'ছ' => "c",
            'জ' | 'ঝ' => "j",
            // ya-phala (after a hasanta) only colours the vowel
            'য' if i > 0 && chars[i - 1] == '\u{09CD}' => "",
            // য় (য plus a dot below) is a glide, not j
            'য' if next == Some('\u{09BC}') => {
                step = 2;
                ""
            }
            'য' => "j",
            'ট' | 'ঠ' | 'ত' | 'থ' | 'ৎ' => "t",
            // ড় and ঢ় (one letter, or the base letter plus a dot below) sound like r
            'ড' | 'ঢ' if next == Some('\u{09BC}') => {
                step = 2;
                "r"
            }
            'ড' | 'ঢ' | 'দ' | 'ধ' => "d",
            'র' | '\u{09DC}' | '\u{09DD}' => "r",
            'প' => "p",
            'ফ' => "f",
            'ব' | 'ভ' => "b",
            'ম' => "m",
            'ল' => "l",
            'শ' | 'ষ' | 'স' => "s",
            'হ' => "h",
            'c' if next == Some('h') => {
                step = 2;
                "c"
            }
            'c' if matches!(next, Some('e' | 'i' | 'y')) => "s",
            'c' | 'k' | 'q' => {
                if next == Some('h') {
                    step = 2;
                }
                "k"
            }
            'x' => "ks",
            's' | 't' | 'g' | 'b' | 'd' | 'p' if next == Some('h') => {
                step = 2;
                match c {
                    's' => "s",
                    't' => "t",
                    'g' => "g",
                    'b' => "b",
                    'd' => "d",
                    _ => "f",
                }
            }
            'b' | 'v' => "b",
            'd' => "d",
            'f' => "f",
            'g' => "g",
            'h' => "h",
            'j' | 'z' => "j",
            'l' => "l",
            'm' => "m",
            'n' => "n",
            'p' => "p",
            'r' => "r",
            's' => "s",
            't' => "t",
            _ => "",
        };
        out.extend(sound.chars());
        i += step;
    }
    out
}

/// How strongly a caption word and a heard word are the same word (0 = not).
fn pair_weight(c: &CapWord, h: &Heard) -> (f32, f32) {
    if c.letters.len() < 2 || h.letters.len() < 2 {
        return (0.0, 0.0);
    }
    if c.latin != h.latin {
        // Different scripts: compare how they sound.
        if c.skeleton.len() < 3 || h.skeleton.len() < 3 {
            return (0.0, 0.0);
        }
        let sim = similarity(&c.skeleton, &h.skeleton);
        return if sim >= 0.75 { (sim * 0.8 * c.skeleton.len().min(h.skeleton.len()) as f32, sim * 0.8) } else { (0.0, 0.0) };
    }
    let sim = similarity(&c.letters, &h.letters);
    if sim >= 0.6 { (sim * c.letters.len().min(h.letters.len()) as f32, sim) } else { (0.0, 0.0) }
}

/// Prefix maximum over heard-word positions, remembering which pair gave it.
struct BestBefore {
    tree: Vec<(f32, u32)>,
}

impl BestBefore {
    fn new(n: usize) -> Self {
        Self { tree: vec![(0.0, u32::MAX); n + 1] }
    }

    /// Best chain ending at a heard word before position `j`.
    fn before(&self, j: usize) -> (f32, u32) {
        let mut best = (0.0, u32::MAX);
        let mut k = j;
        while k > 0 {
            if self.tree[k].0 > best.0 {
                best = self.tree[k];
            }
            k -= k & k.wrapping_neg();
        }
        best
    }

    fn offer(&mut self, j: usize, value: (f32, u32)) {
        let mut k = j + 1;
        while k < self.tree.len() {
            if value.0 > self.tree[k].0 {
                self.tree[k] = value;
            }
            k += k & k.wrapping_neg();
        }
    }
}

fn median(values: &mut [i64]) -> i64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[values.len() / 2]
}

/// The overall shift (heard time minus caption time) as a function of caption time,
/// from words that appear exactly once on each side.
fn trend(caps: &[CapWord], heard: &[Heard]) -> Vec<(i64, i64)> {
    let mut seen_c: HashMap<&[char], (usize, usize)> = HashMap::new();
    for (i, c) in caps.iter().enumerate().filter(|(_, c)| c.letters.len() >= 4) {
        let e = seen_c.entry(&c.letters).or_insert((0, i));
        e.0 += 1;
    }
    let mut seen_h: HashMap<&[char], (usize, usize)> = HashMap::new();
    for (j, h) in heard.iter().enumerate().filter(|(_, h)| h.letters.len() >= 4) {
        let e = seen_h.entry(&h.letters).or_insert((0, j));
        e.0 += 1;
    }
    let mut points: Vec<(i64, i64)> = seen_c
        .iter()
        .filter(|(_, (n, _))| *n == 1)
        .filter_map(|(word, (_, i))| seen_h.get(word).filter(|(n, _)| *n == 1).map(|(_, j)| (caps[*i].at, heard[*j].start - caps[*i].at)))
        .collect();
    points.sort_unstable();
    match points.len() {
        0..=2 => Vec::new(),
        3..=5 => {
            let mut d: Vec<i64> = points.iter().map(|p| p.1).collect();
            vec![(points[0].0, median(&mut d))]
        }
        n => (0..n)
            .map(|k| {
                // Median of the neighbours: one wrong pair can't bend the trend.
                let (a, b) = (k.saturating_sub(3), (k + 4).min(n));
                let mut d: Vec<i64> = points[a..b].iter().map(|p| p.1).collect();
                (points[k].0, median(&mut d))
            })
            .collect(),
    }
}

fn trend_at(knots: &[(i64, i64)], t: i64) -> i64 {
    match knots {
        [] => 0,
        [only] => only.1,
        _ => {
            let k = knots.partition_point(|p| p.0 <= t);
            if k == 0 {
                knots[0].1
            } else if k == knots.len() {
                knots[k - 1].1
            } else {
                let (a, b) = (knots[k - 1], knots[k]);
                a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0).max(1)
            }
        }
    }
}

/// Without pauses to go by (no voice activity information): the heard words' own times.
fn times_without_pauses(heard: &[Heard], anchors: &[Option<usize>]) -> Vec<Option<(i64, i64)>> {
    anchors.iter().map(|a| a.map(|j| (heard[j].start, heard[j].end()))).collect()
}

/// Lays the caption words out in the stretches of speech they belong to.
fn layout(caps: &[CapWord], heard: &[Heard], anchors: &[Option<usize>], regions: &[(i64, i64)]) -> Vec<Option<(i64, i64)>> {
    let n = caps.len();
    let len = |i: usize| caps[i].letters.len() as i64;
    // Which stretch each caption word is in. A word that was heard is in the stretch it
    // was heard in. Words in between that weren't heard: if the heard words on both
    // sides are in one stretch, so are they; if a pause lies between, they go to the
    // side that has room for them at the speaker's pace (the model's own times can't
    // settle it: when it misses words it smears the rest over the gap).
    let mut region: Vec<Option<usize>> = anchors.iter().map(|a| a.map(|j| heard[j].region)).collect();
    let mut across: Vec<(usize, usize, usize, usize)> = Vec::new(); // run [from, to), stretch before, stretch after
    let mut i = 0;
    while i < n {
        if anchors[i].is_some() {
            i += 1;
            continue;
        }
        let from = i;
        while i < n && anchors[i].is_none() {
            i += 1;
        }
        let left = from.checked_sub(1).and_then(|k| anchors[k]).map(|j| heard[j].region);
        let right = if i < n { anchors[i].map(|j| heard[j].region) } else { None };
        match (left, right) {
            (None, None) => {}
            (Some(a), None) => region[from..i].fill(Some(a)),
            (None, Some(b)) => region[from..i].fill(Some(b)),
            (Some(a), Some(b)) if a >= b => region[from..i].fill(Some(a)),
            (Some(a), Some(b)) => across.push((from, i, a, b)),
        }
    }
    if !across.is_empty() {
        // Letters already known to be in each stretch, and the pace: letters per
        // millisecond over the stretches the captions cover.
        let mut filled = vec![0i64; regions.len()];
        for w in 0..n {
            if let Some(k) = region[w] {
                filled[k] += len(w);
            }
        }
        let (first, last) = (region.iter().flatten().min().copied().unwrap_or(0), region.iter().flatten().max().copied().unwrap_or(0));
        let all_letters: i64 = filled.iter().sum::<i64>() + across.iter().map(|&(f, t, _, _)| (f..t).map(len).sum::<i64>()).sum::<i64>();
        let all_time: i64 = regions[first..=last].iter().map(|r| r.1 - r.0).sum::<i64>().max(1);
        let room = |k: usize, filled: &[i64]| (regions[k].1 - regions[k].0) * all_letters / all_time - filled[k];
        for (from, to, a, b) in across {
            // A caption rarely runs across a pause: unheard words of the caption heard
            // before the pause stay before it, those of the caption heard after it stay
            // after it. (The model usually loses the end of a stretch, so that end is
            // the words just before the pause.)
            let (before, after) = (caps[from - 1].cue, caps[to].cue);
            let (mut lo, mut hi) = (from, to);
            if before != after {
                while lo < hi && caps[lo].cue == before {
                    region[lo] = Some(a);
                    filled[a] += len(lo);
                    lo += 1;
                }
                while hi > lo && caps[hi - 1].cue == after {
                    hi -= 1;
                    region[hi] = Some(b);
                    filled[b] += len(hi);
                }
            }
            // What's left (whole captions nobody heard, or one caption that does span
            // the pause) goes where there's room at the speaker's pace: fill the
            // stretch before the pause, then any whole stretches between, the rest
            // after. Whole captions are kept together.
            let whole = before != after;
            let mut w = lo;
            for k in a..b {
                let mut best = (i64::MAX, w);
                let mut taken = 0;
                let rest: i64 = (w..hi).map(len).sum();
                for cut in w..=hi {
                    if !whole || cut == w || cut == hi || caps[cut].cue != caps[cut - 1].cue {
                        let later: i64 = (k + 1..=b).map(|x| room(x, &filled)).sum();
                        let cost = (room(k, &filled) - taken).abs() + (later - (rest - taken)).abs();
                        if cost < best.0 {
                            best = (cost, cut);
                        }
                    }
                    if cut < hi {
                        taken += len(cut);
                    }
                }
                for x in w..best.1 {
                    region[x] = Some(k);
                    filled[k] += len(x);
                }
                w = best.1;
            }
            for x in w..hi {
                region[x] = Some(b);
                filled[b] += len(x);
            }
        }
    }

    let mut times: Vec<Option<(i64, i64)>> = vec![None; n];
    for (k, &(start, end)) in regions.iter().enumerate() {
        let words: Vec<usize> = (0..n).filter(|&i| region[i] == Some(k)).collect();
        if words.is_empty() {
            continue;
        }
        let span = (end - start).max(1);
        let said: Vec<usize> = (0..heard.len()).filter(|&j| heard[j].region == k).collect();
        let cap_letters: i64 = words.iter().map(|&i| len(i)).sum::<i64>().max(1);
        let heard_letters: i64 = said.iter().map(|&j| heard[j].letters.len() as i64).sum();
        if heard_letters * 4 <= cap_letters * 5 {
            // The captions cover what was said here: they are the ruler.
            let mut seen = 0;
            for &i in &words {
                let a = start + span * seen / cap_letters;
                seen += len(i);
                times[i] = Some((a, start + span * seen / cap_letters));
            }
            continue;
        }
        // More was said here than the captions hold (other voices, words left out):
        // the heard words are the ruler, and caption words sit at their heard words.
        let mut at: HashMap<usize, (i64, i64)> = HashMap::new();
        let mut seen = 0;
        for &j in &said {
            let a = start + span * seen / heard_letters;
            seen += heard[j].letters.len() as i64;
            at.insert(j, (a, start + span * seen / heard_letters));
        }
        for &i in &words {
            times[i] = anchors[i].and_then(|j| at.get(&j).copied());
        }
        // Words between two placed words share the time between them.
        let mut w = 0;
        while w < words.len() {
            if times[words[w]].is_some() {
                w += 1;
                continue;
            }
            let run_from = w;
            while w < words.len() && times[words[w]].is_none() {
                w += 1;
            }
            let lo = if run_from > 0 { times[words[run_from - 1]].map_or(start, |t| t.1) } else { start };
            let hi = if w < words.len() { times[words[w]].map_or(end, |t| t.0) } else { end };
            let letters: i64 = words[run_from..w].iter().map(|&i| len(i)).sum::<i64>().max(1);
            let mut seen = 0;
            for &i in &words[run_from..w] {
                let a = lo + (hi - lo).max(0) * seen / letters;
                seen += len(i);
                times[i] = Some((a, lo + (hi - lo).max(0) * seen / letters));
            }
        }
    }
    times
}

/// Moves the captions in `section` (or all) to where their words are heard.
/// Returns one entry per caption, in the order given.
pub fn resync(cues: &[Cue], heard_words: &[Word], speech: &[(i64, i64)], section: Option<(i64, i64)>, duration_ms: i64) -> (Vec<Synced>, Summary) {
    let mut order: Vec<usize> = (0..cues.len()).collect();
    order.sort_by_key(|&i| (cues[i].start_ms, cues[i].end_ms));
    let inside = |c: &Cue| section.is_none_or(|(a, b)| c.start_ms >= a && c.start_ms <= b);

    // Caption words (those with letters), each with where it is now.
    let mut caps: Vec<CapWord> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(order.len());
    for (pos, &ci) in order.iter().enumerate() {
        let cue = &cues[ci];
        let first = caps.len();
        {
            let words: Vec<Vec<char>> = cue.text.split_whitespace().map(letters).filter(|l| !l.is_empty()).collect();
            let total: i64 = words.iter().map(|w| w.len() as i64).sum::<i64>().max(1);
            let span = (cue.end_ms - cue.start_ms).max(1);
            let mut before = 0i64;
            for (w, raw) in words.into_iter().zip(cue.text.split_whitespace().filter(|w| !letters(w).is_empty())) {
                let at = cue.start_ms + span * before / total;
                before += w.len() as i64;
                caps.push(CapWord { cue: pos, latin: is_latin(&w), skeleton: skeleton(raw), letters: w, at });
            }
        }
        ranges.push((first, caps.len()));
    }
    let mut heard: Vec<Heard> = heard_words
        .iter()
        .filter_map(|w| {
            let l = letters(&w.text);
            (!l.is_empty()).then(|| Heard { latin: is_latin(&l), skeleton: skeleton(&w.text), letters: l, start: w.start_ms, end: w.end_ms.max(w.start_ms + 1), region: 0 })
        })
        .collect();
    heard.sort_by_key(|h| h.start);
    for h in &mut heard {
        // The stretch it starts in, or the next one if the model put it in a pause.
        h.region = speech.partition_point(|r| r.1 <= h.start).min(speech.len().saturating_sub(1));
    }

    // Pair caption words with heard words around the trend, keeping the order.
    let knots = trend(&caps, &heard);
    let mut pairs: Vec<(usize, usize, f32, f32, u32)> = Vec::new(); // caption word, heard word, weight, likeness, previous pair
    let mut best = BestBefore::new(heard.len());
    let mut top = (0.0f32, u32::MAX);
    for (i, c) in caps.iter().enumerate() {
        let target = c.at + trend_at(&knots, c.at);
        let lo = heard.partition_point(|h| h.start < target - BAND_MS);
        let hi = heard.partition_point(|h| h.start <= target + BAND_MS);
        let first_of_word = pairs.len();
        for (j, h) in heard.iter().enumerate().take(hi).skip(lo) {
            let (w, sim) = pair_weight(c, h);
            if w <= 0.0 {
                continue;
            }
            // Among equal matches, the one nearest the expected time.
            let near = 1.0 + 0.1 * (1.0 - (h.start - target).abs() as f32 / BAND_MS as f32);
            let (before, prev) = best.before(j);
            pairs.push((i, j, before + w * near, sim, prev));
        }
        for (k, p) in pairs.iter().enumerate().skip(first_of_word) {
            best.offer(p.1, (p.2, k as u32));
            if p.2 > top.0 {
                top = (p.2, k as u32);
            }
        }
    }
    let mut matched: Vec<Option<(usize, f32)>> = vec![None; caps.len()];
    let mut k = top.1;
    while k != u32::MAX {
        let p = pairs[k as usize];
        matched[p.0] = Some((p.1, p.3));
        k = p.4;
    }
    // Sure pairs only: a long, nearly identical word, or part of a run.
    let anchors: Vec<Option<usize>> = (0..caps.len())
        .map(|i| {
            let (j, sim) = matched[i]?;
            let alone = sim >= 0.75 && caps[i].letters.len() >= 4;
            let prev = i > 0 && j > 0 && matched[i - 1].is_some_and(|m| m.0 == j - 1);
            let next = i + 1 < caps.len() && matched[i + 1].is_some_and(|m| m.0 == j + 1);
            (alone || prev || next).then_some(j)
        })
        .collect();

    // Where every caption word is spoken, then each caption's time from its words.
    let times = if speech.is_empty() { times_without_pauses(&heard, &anchors) } else { layout(&caps, &heard, &anchors, speech) };
    let mut out: Vec<Synced> = order
        .iter()
        .map(|&ci| {
            let c = &cues[ci];
            Synced { id: c.id.clone(), start_ms: c.start_ms, end_ms: c.end_ms, old_start_ms: c.start_ms, old_end_ms: c.end_ms, status: Status::Outside, confidence: 0.0 }
        })
        .collect();
    for pos in 0..order.len() {
        let (w0, w1) = ranges[pos];
        if !inside(&cues[order[pos]]) {
            continue;
        }
        out[pos].status = Status::Unmatched;
        let found = (w0..w1).filter(|&i| anchors[i].is_some()).count();
        let total: usize = caps[w0..w1].iter().map(|c| c.letters.len()).sum();
        let hit: usize = (w0..w1).filter(|&i| anchors[i].is_some()).map(|i| caps[i].letters.len()).sum();
        let confidence = if total == 0 { 0.0 } else { hit as f32 / total as f32 };
        out[pos].confidence = confidence;
        let (Some(first), Some(last)) = ((w0..w1).find(|&i| times[i].is_some()), (w0..w1).rev().find(|&i| times[i].is_some())) else { continue };
        // Words of the caption without a time of their own, before and after: room at
        // a normal speaking pace.
        let lead: i64 = caps[w0..first].iter().map(|c| c.letters.len() as i64).sum();
        let trail: i64 = caps[last + 1..w1].iter().map(|c| c.letters.len() as i64).sum();
        let start = times[first].unwrap().0 - lead * 90;
        let end = times[last].unwrap().1 + trail * 90;
        let enough = (found >= 2 && confidence >= 0.3) || (found >= 1 && confidence >= 0.5);
        let placed_whole = lead == 0 && trail == 0;
        if !enough && !placed_whole {
            continue;
        }
        out[pos].start_ms = start;
        out[pos].end_ms = end.max(start + 1);
        // Heard itself, or only placed by the words around it (then: review).
        out[pos].status = if enough { Status::Matched } else { Status::Estimated };
    }

    // A short linger after the last word.
    for s in out.iter_mut().filter(|s| matches!(s.status, Status::Matched | Status::Estimated)) {
        s.end_ms += LINGER_MS;
    }

    // How sure each time is. A caption that begins right after a pause is exact: the
    // pause is in the sound itself, so it takes its own time. One that begins
    // mid-speech is placed by length, good to a few tenths of a second: it takes the
    // shift of the exact captions around it (which also keeps its length), unless it
    // is further off than that, which means it was misplaced on its own.
    // Tiny shifts aren't applied at all, so captions that were right stay untouched.
    let found: Vec<usize> = (0..out.len()).filter(|&p| out[p].status == Status::Matched).collect();
    let raw: Vec<i64> = found.iter().map(|&p| out[p].start_ms - out[p].old_start_ms).collect();
    let exact: Vec<bool> = found.iter().map(|&p| speech.is_empty() || speech.binary_search_by_key(&out[p].start_ms, |r| r.0).is_ok()).collect();
    for (n, &pos) in found.iter().enumerate() {
        let s = &mut out[pos];
        let old_len = s.old_end_ms - s.old_start_ms;
        if !exact[n] {
            let reach = 3.min(n).min(found.len() - 1 - n);
            let window = n - reach..=n + reach;
            let mut sure: Vec<i64> = window.clone().filter(|&m| exact[m]).map(|m| raw[m]).collect();
            let mut around: Vec<i64> = if sure.len() >= 2 { std::mem::take(&mut sure) } else { raw[window].to_vec() };
            let local = median(&mut around);
            if (raw[n] - local).abs() <= AGREE_MS {
                s.start_ms = s.old_start_ms + local;
                s.end_ms = s.start_ms + old_len;
            }
        }
        if (s.start_ms + old_len - s.end_ms).abs() < 400 {
            // Keep the caption's own length when the audio agrees with it.
            s.end_ms = s.start_ms + old_len;
        }
        if (s.start_ms - s.old_start_ms).abs() < SAME_MS {
            s.start_ms = s.old_start_ms;
            s.end_ms = s.old_end_ms;
        }
    }

    // Captions that weren't heard move with their neighbours when those agree.
    let shifts: Vec<Option<i64>> = out.iter().map(|s| (s.status == Status::Matched).then_some(s.start_ms - s.old_start_ms)).collect();
    for pos in 0..out.len() {
        if out[pos].status != Status::Unmatched {
            continue;
        }
        let before = (0..pos).rev().find_map(|p| shifts[p].map(|s| (p, s)));
        let after = (pos + 1..out.len()).find_map(|p| shifts[p].map(|s| (p, s)));
        let at = out[pos].old_start_ms;
        let near = |p: usize| (out[p].old_start_ms - at).abs() <= 30_000;
        let shift = match (before, after) {
            (Some((a, sa)), Some((b, sb))) if (sa - sb).abs() <= 600 => {
                let (ta, tb) = (out[a].old_start_ms, out[b].old_start_ms);
                Some(sa + (sb - sa) * (at - ta) / (tb - ta).max(1))
            }
            (Some((a, sa)), None) if near(a) => Some(sa),
            (None, Some((b, sb))) if near(b) => Some(sb),
            _ => None,
        };
        if let Some(s) = shift.filter(|s| s.abs() >= SAME_MS) {
            out[pos].start_ms += s;
            out[pos].end_ms += s;
            out[pos].status = Status::Estimated;
        }
    }

    let moved = |s: &Synced| s.start_ms != s.old_start_ms || s.end_ms != s.old_end_ms;
    for pos in 0..out.len() {
        if out[pos].status == Status::Outside {
            continue;
        }
        out[pos].start_ms = out[pos].start_ms.clamp(0, (duration_ms - 200).max(0));
        out[pos].end_ms = out[pos].end_ms.clamp(out[pos].start_ms + 200, duration_ms.max(out[pos].start_ms + 200));
        if pos == 0 {
            continue;
        }
        let (head, tail) = out.split_at_mut(pos);
        let (prev, cur) = (&mut head[pos - 1], &mut tail[0]);
        if cur.start_ms < prev.end_ms && (moved(cur) || moved(prev)) && prev.status != Status::Outside {
            // The earlier caption ends where this one begins; if that would leave it
            // too short, this one waits for it instead.
            prev.end_ms = cur.start_ms.max(prev.start_ms + 300);
            if cur.start_ms < prev.end_ms {
                let len = cur.end_ms - cur.start_ms;
                cur.start_ms = prev.end_ms;
                cur.end_ms = cur.end_ms.max(cur.start_ms + len.min(600));
            }
        }
    }

    // The order of the captions is kept. At the edge of a section, a synced caption
    // never crosses into a caption that was left alone: it stops at it, and is marked
    // for review because it couldn't go where it was heard.
    for pos in 0..out.len() {
        if out[pos].status == Status::Outside || !moved(&out[pos]) {
            continue;
        }
        let before = pos.checked_sub(1).filter(|&p| out[p].status == Status::Outside).map(|p| out[p].end_ms);
        let after = out.get(pos + 1).filter(|n| n.status == Status::Outside).map(|n| n.start_ms);
        let s = &mut out[pos];
        let (start, end) = (s.start_ms.max(before.unwrap_or(i64::MIN)), s.end_ms.min(after.unwrap_or(i64::MAX)));
        if start == s.start_ms && end == s.end_ms {
            continue;
        }
        if end - start >= 300 {
            s.start_ms = start;
            s.end_ms = end;
            s.status = Status::Estimated;
        } else {
            // No room between its neighbours: it stays where it was.
            s.start_ms = s.old_start_ms;
            s.end_ms = s.old_end_ms;
            s.status = Status::Unmatched;
        }
    }

    let summary = summarize(&out);
    // Back to the order the captions were given in.
    let mut by_input: Vec<Option<Synced>> = vec![None; cues.len()];
    for (pos, s) in out.into_iter().enumerate() {
        by_input[order[pos]] = Some(s);
    }
    (by_input.into_iter().flatten().collect(), summary)
}

fn summarize(out: &[Synced]) -> Summary {
    let matched: Vec<&Synced> = out.iter().filter(|s| s.status == Status::Matched).collect();
    let count = |st: Status| out.iter().filter(|s| s.status == st).count();
    let changed = out.iter().filter(|s| s.start_ms != s.old_start_ms || s.end_ms != s.old_end_ms).count();
    let shifts: Vec<i64> = matched.iter().map(|s| s.start_ms - s.old_start_ms).collect();
    let mut sorted = shifts.clone();
    sorted.sort_unstable();
    let pick = |q: usize| sorted.get((sorted.len().saturating_sub(1)) * q / 100).copied().unwrap_or(0);
    let edge = |from_end: bool| -> i64 {
        let n = shifts.len().min(3);
        let mut part: Vec<i64> = if from_end { shifts[shifts.len() - n..].to_vec() } else { shifts[..n].to_vec() };
        median(&mut part)
    };
    let drift = steady_drift(&matched, &shifts);
    // For a drift, the ends of the fitted line say it best.
    let (first, last) = drift.unwrap_or((edge(false), edge(true)));
    let max = shifts.iter().copied().max_by_key(|s| s.abs()).unwrap_or(0);
    let kind = if matched.is_empty() {
        Kind::NoMatch
    } else if shifts.iter().all(|s| s.abs() < 150) {
        Kind::InSync
    } else if pick(90) - pick(10) <= 400 && pick(50).abs() >= 150 {
        Kind::Offset
    } else if drift.is_some() {
        Kind::Drift
    } else {
        Kind::Mixed
    };
    Summary { kind, matched: matched.len(), estimated: count(Status::Estimated), unmatched: count(Status::Unmatched), changed, median_shift_ms: pick(50), first_shift_ms: first, last_shift_ms: last, max_shift_ms: max }
}

/// If the shifts follow a straight line that rises or falls by at least 0.4 s: the
/// line's value at the first and at the last caption.
fn steady_drift(matched: &[&Synced], shifts: &[i64]) -> Option<(i64, i64)> {
    if matched.len() < 4 {
        return None;
    }
    let n = matched.len() as f64;
    let xs: Vec<f64> = matched.iter().map(|s| s.old_start_ms as f64).collect();
    let ys: Vec<f64> = shifts.iter().map(|&s| s as f64).collect();
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let slope = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / sxx;
    let resid = (xs.iter().zip(&ys).map(|(x, y)| (y - (my + slope * (x - mx))).powi(2)).sum::<f64>() / n).sqrt();
    let at = |x: f64| (my + slope * (x - mx)).round() as i64;
    let rise = slope * (xs[xs.len() - 1] - xs[0]);
    (rise.abs() >= 400.0 && resid <= 350.0).then(|| (at(xs[0]), at(xs[xs.len() - 1])))
}

/// Stretches of audio around captions that weren't found, for a second, more careful
/// listen. Merged where they touch.
pub fn unmatched_spans(synced: &[Synced], pad_ms: i64, duration_ms: i64) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = synced
        .iter()
        .filter(|s| matches!(s.status, Status::Unmatched | Status::Estimated))
        .map(|s| ((s.start_ms - pad_ms).max(0), (s.end_ms + pad_ms).min(duration_ms)))
        .filter(|(a, b)| b > a)
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTENCES: &[&str] = &[
        "আসসালামু আলাইকুম এটিএন বাংলা সংবাদে সবাইকে স্বাগত",
        "টানা কয়েক সপ্তাহের আলোচনা দর কষাকষি শেষে",
        "তৈরি পোশাক শিল্পের শ্রমিকদের নতুন মজুরি নির্ধারণ",
        "আগামী ডিসেম্বর থেকে কার্যকর হবে নতুন কাঠামো",
        "দ্রুতই প্রকাশ করা হবে গেজেট",
        "এদিকে পোশাক শ্রমিকদের মধ্যে দেখা দিয়েছে মতভেদ",
        "সকালে কিছু কারখানা খুলে দেওয়া হলেও কর্মস্থল ছাড়ছেন",
        "এতে সড়কটিতে কিছুক্ষণের জন্য যান চলাচল বন্ধ থাকে",
    ];

    /// The truth: each sentence spoken for 3 s, one every 4 s, starting at 2 s.
    fn truth() -> Vec<(i64, i64, &'static str)> {
        SENTENCES.iter().enumerate().map(|(k, s)| (2_000 + k as i64 * 4_000, 5_000 + k as i64 * 4_000, *s)).collect()
    }

    /// What the offline model heard: every word at its true time, some misspelled.
    fn heard() -> Vec<Word> {
        let mut out = Vec::new();
        for (a, b, text) in truth() {
            let words: Vec<&str> = text.split_whitespace().collect();
            let step = (b - a) / words.len() as i64;
            for (k, w) in words.iter().enumerate() {
                // The model drops the last letter of every third word.
                let said: String = if k % 3 == 2 { w.chars().take(w.chars().count() - 1).collect() } else { (*w).to_string() };
                out.push(Word { start_ms: a + k as i64 * step, end_ms: a + (k as i64 + 1) * step - 20, text: format!(" {said}"), confidence: 1.0 });
            }
        }
        out
    }

    fn captions(shift: impl Fn(i64) -> i64) -> Vec<Cue> {
        truth().into_iter().enumerate().map(|(k, (a, b, t))| Cue { id: format!("c{k}"), start_ms: shift(a), end_ms: shift(b), text: format!("{t}।"), check: Vec::new(), timing_check: false }).collect()
    }

    fn worst_start_error(synced: &[Synced]) -> i64 {
        synced.iter().zip(truth()).map(|(s, (a, _, _))| (s.start_ms - a).abs()).max().unwrap()
    }

    #[test]
    fn a_steady_offset_is_removed() {
        let cues = captions(|t| t + 2_300);
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        assert_eq!(sum.kind, Kind::Offset);
        assert!((sum.median_shift_ms + 2_300).abs() <= 80, "{sum:?}");
        assert_eq!(sum.matched, 8);
        assert!(worst_start_error(&synced) <= 80, "{synced:?}");
        // Same captions, same order, same ids.
        assert_eq!(synced.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), cues.iter().map(|c| c.id.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn a_growing_drift_is_removed() {
        // 6% too slow: 0.12 s late at the start, 1.9 s late by the end.
        let cues = captions(|t| t * 106 / 100);
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        assert_eq!(sum.kind, Kind::Drift, "{sum:?}");
        assert!(sum.last_shift_ms < sum.first_shift_ms - 1_000, "{sum:?}");
        assert!(worst_start_error(&synced) <= 80, "{synced:?}");
    }

    #[test]
    fn captions_already_in_place_are_left_exactly_as_they_were() {
        let cues = captions(|t| t);
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        assert_eq!(sum.kind, Kind::InSync);
        assert_eq!(sum.changed, 0);
        assert!(synced.iter().zip(&cues).all(|(s, c)| s.start_ms == c.start_ms && s.end_ms == c.end_ms));
    }

    #[test]
    fn one_misplaced_caption_moves_and_the_rest_stay() {
        let mut cues = captions(|t| t);
        cues[3].start_ms -= 3_500;
        cues[3].end_ms -= 3_500;
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        assert_eq!(sum.kind, Kind::Mixed);
        assert_eq!(sum.changed, 1, "{synced:?}");
        let fixed = synced.iter().find(|s| s.id == "c3").unwrap();
        assert!((fixed.start_ms - truth()[3].0).abs() <= 80, "{fixed:?}");
    }

    #[test]
    fn a_caption_that_was_not_heard_moves_with_its_neighbours_and_is_flagged() {
        let mut cues = captions(|t| t + 1_800);
        cues[4].text = "সম্পূর্ণ ভিন্ন কিছু শব্দ এখানে।".into();
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        let odd = synced.iter().find(|s| s.id == "c4").unwrap();
        assert_eq!(odd.status, Status::Estimated);
        assert!((odd.start_ms - odd.old_start_ms + 1_800).abs() <= 150, "{odd:?}");
        assert_eq!(sum.estimated, 1);
    }

    #[test]
    fn a_caption_that_was_not_heard_stays_when_its_neighbours_disagree() {
        let mut cues = captions(|t| t);
        // Before it: 2 s late. After it: in sync. Nothing says where it belongs.
        for c in &mut cues[..3] {
            c.start_ms += 2_000;
            c.end_ms += 2_000;
        }
        cues[3].text = "সম্পূর্ণ ভিন্ন কিছু শব্দ এখানে।".into();
        let before = (cues[3].start_ms, cues[3].end_ms);
        let (synced, _) = resync(&cues, &heard(), &[], None, 40_000);
        let odd = synced.iter().find(|s| s.id == "c3").unwrap();
        assert_eq!(odd.status, Status::Unmatched);
        assert_eq!((odd.start_ms, odd.end_ms), before);
    }

    #[test]
    fn only_the_chosen_section_changes() {
        let cues = captions(|t| t + 1_500);
        let (synced, sum) = resync(&cues, &heard(), &[], Some((15_000, 26_000)), 40_000);
        let touched: Vec<&str> = synced.iter().filter(|s| s.status != Status::Outside).map(|s| s.id.as_str()).collect();
        assert_eq!(touched, vec!["c3", "c4", "c5"]);
        assert!(synced.iter().filter(|s| s.status == Status::Outside).all(|s| s.start_ms == s.old_start_ms && s.end_ms == s.old_end_ms));
        // All three were found; the one at the section's edge had to stop at the late,
        // untouched caption before it, so it counts as "to review".
        assert_eq!(sum.matched + sum.estimated, 3);
        assert!(sum.matched >= 2, "{sum:?}");
    }

    #[test]
    fn a_section_never_reorders_captions_at_its_edges() {
        // Everything is 2.5 s late, only the middle is synced: its first caption's real
        // time lies inside the (late, untouched) caption before it.
        let cues = captions(|t| t + 2_500);
        let (synced, _) = resync(&cues, &heard(), &[], Some((15_000, 26_000)), 40_000);
        let mut order: Vec<(i64, &str)> = synced.iter().map(|s| (s.start_ms, s.id.as_str())).collect();
        order.sort();
        assert_eq!(order.iter().map(|o| o.1).collect::<Vec<_>>(), cues.iter().map(|c| c.id.as_str()).collect::<Vec<_>>());
        let first = synced.iter().find(|s| s.status != Status::Outside).unwrap();
        let before = &synced[synced.iter().position(|s| s.id == first.id).unwrap() - 1];
        assert!(first.start_ms >= before.end_ms, "{first:?} {before:?}");
        assert_ne!(first.status, Status::Matched, "a caption that couldn't go where it was heard is for review");
    }

    #[test]
    fn nothing_is_guessed_when_the_words_are_not_in_the_audio() {
        let cues: Vec<Cue> = (0..4).map(|k| Cue { id: format!("x{k}"), start_ms: k * 4_000, end_ms: k * 4_000 + 3_000, text: "The quick brown fox jumps over lazy dogs".into(), check: Vec::new(), timing_check: false }).collect();
        let (synced, sum) = resync(&cues, &heard(), &[], None, 40_000);
        assert_eq!(sum.kind, Kind::NoMatch);
        assert_eq!(sum.changed, 0);
        assert!(synced.iter().all(|s| s.status == Status::Unmatched));
    }

    #[test]
    fn english_words_match_their_bangla_spelling() {
        assert_eq!(skeleton("expensive"), skeleton("এক্সপেন্সিভ"));
        assert_eq!(skeleton("highway"), vec!['h', 'g']);
        // A caption written with Latin-script English words, heard in Bengali script.
        let heard = vec![
            Word { start_ms: 5_000, end_ms: 5_400, text: " বাট".into(), confidence: 1.0 },
            Word { start_ms: 5_400, end_ms: 6_100, text: " এক্সপেন্সিভ".into(), confidence: 1.0 },
            Word { start_ms: 6_100, end_ms: 6_500, text: " জিনিস".into(), confidence: 1.0 },
            Word { start_ms: 6_500, end_ms: 6_900, text: " রাখা".into(), confidence: 1.0 },
            Word { start_ms: 6_900, end_ms: 7_500, text: " এভয়েড".into(), confidence: 1.0 },
            Word { start_ms: 7_500, end_ms: 7_900, text: " করবেন".into(), confidence: 1.0 },
        ];
        let cues = vec![Cue { id: "m".into(), start_ms: 1_000, end_ms: 3_900, text: "but expensive জিনিস রাখা avoid করবেন।".into(), check: Vec::new(), timing_check: false }];
        let (synced, _) = resync(&cues, &heard, &[], None, 20_000);
        assert_eq!(synced[0].status, Status::Matched);
        assert!((synced[0].start_ms - 5_000).abs() <= 450, "{synced:?}");
    }

    /// Speech as the pause detector sees it: sentences 0-1 run together, then pauses.
    fn regions() -> Vec<(i64, i64)> {
        vec![(2_000, 9_000), (10_000, 13_000), (14_000, 17_000), (18_000, 21_000), (22_000, 25_000), (26_000, 29_000), (30_000, 33_000)]
    }

    /// The same sentences with no pause between the first two (2-9 s as one stretch).
    fn run_on_truth() -> Vec<(i64, i64, &'static str)> {
        let mut t = truth();
        t[0] = (2_000, 5_600, SENTENCES[0]);
        t[1] = (5_600, 9_000, SENTENCES[1]);
        t
    }

    fn heard_from(truth: &[(i64, i64, &str)], smear: bool, drop_tail_of: Option<usize>) -> Vec<Word> {
        let mut out = Vec::new();
        for (n, (a, b, text)) in truth.iter().enumerate() {
            let mut words: Vec<&str> = text.split_whitespace().collect();
            if drop_tail_of == Some(n) {
                words.truncate(words.len() - 3);
            }
            // Smeared: the model spreads what it heard over the whole stretch, as it does.
            let step = (b - a) / words.len() as i64;
            for (k, w) in words.iter().enumerate() {
                let late = if smear { 900 } else { 0 };
                // A word is always heard inside its own stretch, however late.
                let start = (a + k as i64 * step + late).min(b - 100);
                out.push(Word { start_ms: start, end_ms: start + 80, text: format!(" {w}"), confidence: 1.0 });
            }
        }
        out
    }

    #[test]
    fn pauses_set_the_times_not_the_models_word_times() {
        // Every heard word is 0.9 s late inside its stretch (real behaviour). Captions
        // still land on the pauses, and the run-on pair is split by length.
        let truth = run_on_truth();
        let cues: Vec<Cue> = truth.iter().enumerate().map(|(k, (a, b, t))| Cue { id: format!("c{k}"), start_ms: a + 2_000, end_ms: b + 2_000, text: (*t).into(), check: Vec::new(), timing_check: false }).collect();
        let (synced, sum) = resync(&cues, &heard_from(&truth, true, None), &regions(), None, 40_000);
        assert_eq!(sum.matched, 8);
        // Captions that begin after a pause: exactly on it. The one that begins mid-speech
        // (c1): placed by length, within half a second.
        for (k, (s, (a, _, _))) in synced.iter().zip(&truth).enumerate() {
            let slack = if k == 1 { 500 } else { 100 };
            assert!((s.start_ms - a).abs() <= slack, "{} at {} should be {a}", s.id, s.start_ms);
        }
    }

    #[test]
    fn words_the_model_missed_stay_on_their_side_of_the_pause() {
        // The model drops the last three words of sentence 3 (before the pause at 17 s).
        // They belong before the pause, so sentence 4 still starts right at 18 s.
        let truth = run_on_truth();
        let cues: Vec<Cue> = truth.iter().enumerate().map(|(k, (a, b, t))| Cue { id: format!("c{k}"), start_ms: a - 1_500, end_ms: b - 1_500, text: (*t).into(), check: Vec::new(), timing_check: false }).collect();
        let (synced, _) = resync(&cues, &heard_from(&truth, false, Some(3)), &regions(), None, 40_000);
        assert!((synced[4].start_ms - truth[4].0).abs() <= 100, "{:?}", synced[4]);
        assert!((synced[3].start_ms - truth[3].0).abs() <= 100, "{:?}", synced[3]);
        assert!(synced[3].end_ms <= truth[4].0, "{:?}", synced[3]);
    }

    #[test]
    fn second_listen_covers_the_captions_that_were_not_found() {
        let s = |a, b, status| Synced { id: String::new(), start_ms: a, end_ms: b, old_start_ms: a, old_end_ms: b, status, confidence: 0.0 };
        let spans = unmatched_spans(&[s(1_000, 3_000, Status::Matched), s(10_000, 12_000, Status::Unmatched), s(13_000, 15_000, Status::Estimated), s(40_000, 42_000, Status::Unmatched)], 2_000, 43_000);
        assert_eq!(spans, vec![(8_000, 17_000), (38_000, 43_000)]);
    }
}
