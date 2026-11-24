//! SRT timestamps: `HH:MM:SS,mmm`.

/// Formats milliseconds as an SRT timestamp. Negative input clamps to zero.
/// Hours are zero-padded to two digits and grow past 99 if needed.
pub fn format_srt(ms: i64) -> String {
    let ms = ms.max(0);
    let hours = ms / 3_600_000;
    let minutes = (ms / 60_000) % 60;
    let seconds = (ms / 1_000) % 60;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// Parses `HH:MM:SS,mmm` (a `.` separator is also accepted, as some tools write it).
/// Fractions shorter than three digits are scaled (`,5` = 500 ms).
pub fn parse_srt(input: &str) -> Option<i64> {
    let input = input.trim();
    let (clock, frac) = match input.rfind([',', '.']) {
        Some(i) => (&input[..i], &input[i + 1..]),
        None => (input, "0"),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() != 3 || frac.is_empty() || frac.len() > 3 {
        return None;
    }
    let num = |s: &str| -> Option<i64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let (h, m, s) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
    if m > 59 || s > 59 {
        return None;
    }
    let f = num(frac)?;
    let millis = match frac.len() {
        1 => f * 100,
        2 => f * 10,
        _ => f,
    };
    Some(h * 3_600_000 + m * 60_000 + s * 1_000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_zero_and_typical_values() {
        assert_eq!(format_srt(0), "00:00:00,000");
        assert_eq!(format_srt(1), "00:00:00,001");
        assert_eq!(format_srt(61_234), "00:01:01,234");
        assert_eq!(format_srt(3_600_000), "01:00:00,000");
        assert_eq!(format_srt(3_599_999), "00:59:59,999");
    }

    #[test]
    fn formats_long_durations_and_clamps_negative() {
        assert_eq!(format_srt(100 * 3_600_000 + 5), "100:00:00,005");
        assert_eq!(format_srt(-40), "00:00:00,000");
    }

    #[test]
    fn parses_comma_and_dot_separators() {
        assert_eq!(parse_srt("00:01:01,234"), Some(61_234));
        assert_eq!(parse_srt("00:01:01.234"), Some(61_234));
        assert_eq!(parse_srt(" 01:00:00,000 "), Some(3_600_000));
        assert_eq!(parse_srt("00:00:01,5"), Some(1_500));
        assert_eq!(parse_srt("00:00:01,05"), Some(1_050));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["", "1:2", "00:60:00,000", "00:00:60,000", "aa:00:00,000", "00:00:00,1234", "00:00:00,"] {
            assert_eq!(parse_srt(bad), None, "{bad:?} should be rejected");
        }
    }

    #[test]
    fn round_trips_every_millisecond_boundary_case() {
        for ms in [0, 999, 1_000, 59_999, 60_000, 3_599_999, 3_600_000, 86_399_999, 360_000_123] {
            assert_eq!(parse_srt(&format_srt(ms)), Some(ms));
        }
    }
}
