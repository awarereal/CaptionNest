//! Short dips in loudness inside continuous speech, used to place caption edges that fall
//! mid-speech.
//!
//! The voice-activity model only reports pauses of about half a second or more, and a
//! caption that begins inside continuous speech is otherwise placed by the length of its
//! text: good to about 0.3-0.45 s, which reads as captions randomly early or late. People
//! still take a short breath between clauses: the loudness drops for a tenth of a second
//! or so. On exactly timed Bangla speech, captions that began after such a dip (62-133 ms
//! long) were placed with a median error of 204 ms by text length alone.
//!
//! A caption edge is moved to a dip only when
//!   - the caption before it ends a clause or sentence (`,` `।` `?` `!` …), where speakers
//!     breathe; splits made only for line length often have no dip, and
//!   - the dip is at least `MIN_DIP_MS` long (stop consonants inside a word close the
//!     mouth for less), and
//!   - it is within `REACH_MS` of where the caption was placed.
//! Among several, a longer and nearer dip wins.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::captions::Cue;
use crate::media::wav;

/// Loudness is measured in 10 ms frames.
const FRAME_MS: i64 = 10;
/// A dip shorter than this is a consonant, not a breath.
pub const MIN_DIP_MS: i64 = 80;
/// How far a caption edge may move to reach a dip.
pub const REACH_MS: i64 = 450;
/// A frame this far below the stretch's typical speech level is quiet.
const DROP_DB: f32 = 18.0;
/// Edges this close to a pause already sit on it.
const AT_PAUSE_MS: i64 = 60;
/// Time a caption stays after its last word before a dip (as elsewhere).
const LINGER_MS: i64 = 150;

/// Loudness (dB) of every 10 ms frame of a 16 kHz mono 16-bit WAV.
pub fn frame_levels(path: &Path) -> std::io::Result<Vec<f32>> {
    let info = wav::read_info(path)?;
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(info.data_offset))?;
    let frame_bytes = (info.sample_rate as usize * FRAME_MS as usize / 1000) * info.channels as usize * 2;
    let mut levels = Vec::with_capacity(info.data_len as usize / frame_bytes.max(1) + 1);
    let mut buf = vec![0u8; frame_bytes * 1000];
    let mut left = info.data_len as usize;
    let mut carry: Vec<u8> = Vec::new();
    while left > 0 {
        let want = buf.len().min(left);
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        left -= n;
        carry.extend_from_slice(&buf[..n]);
        let whole = carry.len() / frame_bytes * frame_bytes;
        for frame in carry[..whole].chunks_exact(frame_bytes) {
            levels.push(level_db(frame));
        }
        carry.drain(..whole);
    }
    Ok(levels)
}

fn level_db(frame: &[u8]) -> f32 {
    let mut sum = 0f64;
    let n = frame.len() / 2;
    for s in frame.chunks_exact(2) {
        let v = i16::from_le_bytes([s[0], s[1]]) as f64;
        sum += v * v;
    }
    let rms = (sum / n.max(1) as f64).sqrt();
    (20.0 * (rms + 1.0).log10()) as f32
}

/// Dips (start, end in ms) inside the speech regions: runs of frames well below the
/// region's typical speech level, at least `MIN_DIP_MS` long, not touching its edges.
pub fn find(levels: &[f32], speech: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut dips = Vec::new();
    for &(a, b) in speech {
        let (fa, fb) = ((a / FRAME_MS).max(0) as usize, ((b / FRAME_MS) as usize).min(levels.len()));
        if fb <= fa + 20 {
            continue;
        }
        let region = &levels[fa..fb];
        let mut sorted = region.to_vec();
        sorted.sort_by(|x, y| x.total_cmp(y));
        // The typical level of the voice in this stretch (louder frames, not the quiet ones).
        let speech_level = sorted[sorted.len() * 8 / 10];
        let quiet = speech_level - DROP_DB;
        let mut run_start: Option<usize> = None;
        for (i, &l) in region.iter().enumerate().chain(std::iter::once((region.len(), &f32::MAX))) {
            match (l < quiet, run_start) {
                (true, None) => run_start = Some(i),
                (false, Some(s)) => {
                    let (start_ms, end_ms) = ((fa + s) as i64 * FRAME_MS, (fa + i) as i64 * FRAME_MS);
                    let away_from_edges = start_ms > a + AT_PAUSE_MS && end_ms < b - AT_PAUSE_MS;
                    if end_ms - start_ms >= MIN_DIP_MS && away_from_edges {
                        dips.push((start_ms, end_ms));
                    }
                    run_start = None;
                }
                _ => {}
            }
        }
    }
    dips
}

/// Whether a caption's text ends a clause or sentence (where speakers take a breath).
fn ends_clause(text: &str) -> bool {
    let t = text.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '”' | '’' | ')' | ']'));
    t.ends_with([',', '.', '।', '॥', '?', '!', ';', ':', '…', '،', '؟', '،', '-', '—'])
}

/// Moves caption starts that fall inside continuous speech onto the dip they belong to,
/// and ends the caption before at the start of that dip. `cues` must be sorted by start.
/// Returns how many captions moved.
pub fn snap(cues: &mut [Cue], dips: &[(i64, i64)], speech: &[(i64, i64)]) -> usize {
    if dips.is_empty() {
        return 0;
    }
    let at_pause = |ms: i64| speech.iter().any(|&(a, _)| (ms - a).abs() <= AT_PAUSE_MS);
    let mut moved = 0;
    for i in 1..cues.len() {
        let start = cues[i].start_ms;
        if at_pause(start) || !ends_clause(&cues[i - 1].text) || cues[i].text.trim().is_empty() {
            continue;
        }
        // The dip must stay between the two captions' other edges.
        let lower = cues[i - 1].start_ms + 300;
        let upper = cues[i].end_ms - 300;
        let best = dips
            .iter()
            .filter(|&&(ds, de)| (de - start).abs() <= REACH_MS && ds >= lower && de <= upper)
            .max_by_key(|&&(ds, de)| (de - ds) * 2 - (de - start).abs());
        let Some(&(ds, de)) = best else { continue };
        if (de - start).abs() < 40 {
            continue;
        }
        cues[i].start_ms = de;
        let prev = &mut cues[i - 1];
        prev.end_ms = (ds + LINGER_MS).min(de).max(prev.start_ms + 300);
        moved += 1;
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 ms frames: speech at 70 dB with quiet stretches at 30 dB.
    fn levels(total_ms: i64, quiet: &[(i64, i64)]) -> Vec<f32> {
        (0..total_ms / FRAME_MS)
            .map(|f| {
                let ms = f * FRAME_MS;
                if quiet.iter().any(|&(a, b)| ms >= a && ms < b) { 30.0 } else { 70.0 }
            })
            .collect()
    }

    #[test]
    fn finds_breaths_but_not_consonants() {
        // One stretch of speech 0-10 s: a 120 ms breath at 4 s, a 40 ms closure at 6 s.
        let l = levels(10_000, &[(4_000, 4_120), (6_000, 6_040)]);
        assert_eq!(find(&l, &[(0, 10_000)]), vec![(4_000, 4_120)]);
    }

    #[test]
    fn a_mid_speech_caption_moves_to_its_breath() {
        let dips = vec![(4_000, 4_120)];
        let mut cues = vec![Cue::new(1_000, 4_400, "আমরা আজ সমুদ্রে যাব,"), Cue::new(4_400, 7_000, "তারপর পাহাড়ে উঠব।")];
        assert_eq!(snap(&mut cues, &dips, &[(1_000, 10_000)]), 1);
        assert_eq!(cues[1].start_ms, 4_120);
        assert_eq!(cues[0].end_ms, 4_120);
    }

    #[test]
    fn length_splits_and_pause_starts_stay() {
        let dips = vec![(4_000, 4_120)];
        // No clause end before the second caption: a split for length, left alone.
        let mut cues = vec![Cue::new(1_000, 4_400, "আমরা আজ সমুদ্রে"), Cue::new(4_400, 7_000, "যাব তারপর পাহাড়ে উঠব।")];
        assert_eq!(snap(&mut cues, &dips, &[(1_000, 10_000)]), 0);
        // A caption that starts at a pause is exact already.
        let mut cues = vec![Cue::new(1_000, 3_000, "আমরা যাব,"), Cue::new(4_420, 7_000, "তারপর উঠব।")];
        assert_eq!(snap(&mut cues, &dips, &[(1_000, 3_000), (4_400, 10_000)]), 0);
        // Too far away.
        let mut cues = vec![Cue::new(1_000, 5_000, "আমরা যাব,"), Cue::new(5_000, 8_000, "তারপর উঠব।")];
        assert_eq!(snap(&mut cues, &dips, &[(1_000, 10_000)]), 0);
    }
}
