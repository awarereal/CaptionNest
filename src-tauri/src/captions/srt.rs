//! SRT writer (for export) and reader (for tests and importing existing SRT files).
//!
//! Output format, chosen for CapCut Desktop and Web:
//! - UTF-8 without a byte-order mark
//! - CRLF line endings
//! - `n` / `HH:MM:SS,mmm --> HH:MM:SS,mmm` / text lines / blank line

use super::{timefmt, Cue};

const EOL: &str = "\r\n";

/// Normalizes cue text for SRT: unify line endings, trim each line, drop empty lines
/// (an empty line would end the cue early in every SRT reader). Intentional single
/// line breaks are kept.
pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serializes cues in the given order, numbering them from 1.
pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&(i + 1).to_string());
        out.push_str(EOL);
        out.push_str(&timefmt::format_srt(cue.start_ms));
        out.push_str(" --> ");
        out.push_str(&timefmt::format_srt(cue.end_ms));
        out.push_str(EOL);
        out.push_str(&normalize_text(&cue.text).replace('\n', EOL));
        out.push_str(EOL);
        out.push_str(EOL);
    }
    out
}

/// Parses SRT text. Tolerates a BOM, LF or CRLF endings, missing/odd cue numbers and
/// extra blank lines. Returns an error message naming the first bad block.
pub fn parse_srt(input: &str) -> Result<Vec<Cue>, String> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input).replace("\r\n", "\n");
    // Lines holding only spaces or tabs count as blank (common in hand-edited files).
    let input = input.lines().map(|l| if l.trim().is_empty() { "" } else { l }).collect::<Vec<_>>().join("\n");
    let mut cues = Vec::new();
    for (block_no, block) in input.split("\n\n").enumerate() {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let timing_at = lines.iter().position(|l| l.contains("-->"));
        let Some(t) = timing_at else {
            return Err(format!("Block {} has no timing line", block_no + 1));
        };
        let (a, b) = lines[t].split_once("-->").unwrap();
        // Ignore position settings some tools append after the end time.
        let b = b.trim().split_whitespace().next().unwrap_or("");
        let start = timefmt::parse_srt(a)
            .ok_or_else(|| format!("Block {} has a bad start time", block_no + 1))?;
        let end = timefmt::parse_srt(b)
            .ok_or_else(|| format!("Block {} has a bad end time", block_no + 1))?;
        let text = lines[t + 1..].join("\n");
        cues.push(Cue::new(start, end, text));
    }
    Ok(cues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_separator_line_with_spaces_still_separates_captions() {
        let text = "1\n00:00:01,000 --> 00:00:02,000\nOne\n \n2\n00:00:03,000 --> 00:00:04,000\nTwo\n";
        let cues = parse_srt(text).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].text, "Two");
    }

    fn cue(s: i64, e: i64, t: &str) -> Cue {
        Cue { check: Vec::new(), id: "x".into(), start_ms: s, end_ms: e, text: t.into() }
    }

    #[test]
    fn writes_numbered_crlf_blocks() {
        let srt = to_srt(&[cue(0, 1_500, "Hello"), cue(2_000, 3_250, "World")]);
        assert_eq!(
            srt,
            "1\r\n00:00:00,000 --> 00:00:01,500\r\nHello\r\n\r\n2\r\n00:00:02,000 --> 00:00:03,250\r\nWorld\r\n\r\n"
        );
        assert!(!srt.starts_with('\u{feff}'), "no BOM");
    }

    #[test]
    fn keeps_intentional_line_breaks_and_removes_blank_lines() {
        let srt = to_srt(&[cue(0, 1_000, "  first line \n\n\nsecond line  ")]);
        assert!(srt.contains("first line\r\nsecond line\r\n\r\n"));
        assert!(!srt.contains("\r\n\r\n\r\n"));
    }

    #[test]
    fn preserves_bangla_arabic_hindi_and_emoji_text() {
        let texts = ["আমি বাংলায় কথা বলি।", "مرحبا، كيف حالك؟", "नमस्ते दुनिया", "Café 🎬"];
        let cues: Vec<Cue> = texts.iter().enumerate().map(|(i, t)| cue(i as i64 * 1000, i as i64 * 1000 + 900, t)).collect();
        let srt = to_srt(&cues);
        let back = parse_srt(&srt).unwrap();
        let got: Vec<&str> = back.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(got, texts);
    }

    #[test]
    fn round_trips_timing_exactly() {
        let cues = vec![cue(0, 1, "a"), cue(59_999, 61_001, "b\nc"), cue(3_600_000, 3_600_500, "d")];
        let back = parse_srt(&to_srt(&cues)).unwrap();
        for (a, b) in cues.iter().zip(&back) {
            assert_eq!((a.start_ms, a.end_ms, &a.text), (b.start_ms, b.end_ms, &b.text));
        }
    }

    #[test]
    fn parses_bom_lf_and_trailing_settings() {
        let input = "\u{feff}1\n00:00:01,000 --> 00:00:02,000 X1:10\nHi\n\n\n2\n00:00:03.000 --> 00:00:04.000\nThere\n";
        let cues = parse_srt(input).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].end_ms, 2_000);
        assert_eq!(cues[1].start_ms, 3_000);
    }

    #[test]
    fn reports_bad_blocks() {
        assert!(parse_srt("1\nnot a time\nHi\n").is_err());
        assert!(parse_srt("1\n00:00:xx,000 --> 00:00:01,000\nHi\n").is_err());
    }

    #[test]
    fn empty_input_gives_no_cues() {
        assert_eq!(to_srt(&[]), "");
        assert!(parse_srt("").unwrap().is_empty());
    }
}
