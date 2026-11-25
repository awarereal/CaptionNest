//! Script repair for Bengali-script languages.
//!
//! Whisper often writes Bangla speech partly (or fully) in Devanagari letters, the
//! script it saw most for related sounds. The Devanagari and Bengali Unicode blocks are
//! laid out in parallel (both derive from ISCII), so a stray Devanagari letter maps to
//! the Bengali letter 0x80 code points later. On CaptionNest's FLEURS Bangla benchmark
//! this cut the large-v3-turbo character error rate from 68.6% to 44.4%.

/// Languages written in Bengali script.
pub fn uses_bengali_script(lang: &str) -> bool {
    matches!(lang, "bn" | "as")
}

/// Maps Devanagari letters to their Bengali counterparts. Danda (।) and double danda
/// (॥) are shared by both scripts and stay as they are; code points with no Bengali
/// counterpart are left unchanged.
pub fn devanagari_to_bengali(text: &str) -> String {
    text.chars()
        .map(|c| {
            let cp = c as u32;
            // Whisper sometimes emits code points that are unassigned in the Bengali block
            // (Devanagari letters shifted into it). They render as empty boxes, so map them
            // to the letter Bengali uses for that sound.
            match cp {
                0x09B5 => return '\u{09AC}', // "va" -> ব
                0x09B1 => return '\u{09B0}', // "rra" -> র
                0x09B4 => return '\u{09B2}', // "llla" -> ল
                0x09B3 => return '\u{09B2}', // "lla" -> ল
                _ => {}
            }
            if !(0x0900..=0x097F).contains(&cp) || cp == 0x0964 || cp == 0x0965 {
                return c;
            }
            // Bengali has no separate "va"; it is written with ba.
            if cp == 0x0935 {
                return '\u{09AC}';
            }
            match char::from_u32(cp + 0x80) {
                Some(b) if is_assigned_bengali(b) => b,
                _ => c,
            }
        })
        .collect()
}

fn is_assigned_bengali(c: char) -> bool {
    // Assigned code points in the Bengali block (Unicode 15).
    matches!(c as u32,
        0x0980..=0x0983 | 0x0985..=0x098C | 0x098F..=0x0990 | 0x0993..=0x09A8 |
        0x09AA..=0x09B0 | 0x09B2 | 0x09B6..=0x09B9 | 0x09BC..=0x09C4 | 0x09C7..=0x09C8 |
        0x09CB..=0x09CE | 0x09D7 | 0x09DC..=0x09DD | 0x09DF..=0x09E3 | 0x09E6..=0x09FE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_devanagari_bangla_to_bengali_script() {
        // Real Whisper output for the FLEURS sentence "এটিকে মঙ্গলবার কেটে ফেলার কথা ছিল".
        assert_eq!(devanagari_to_bengali("एটিকে"), "এটিকে");
        assert_eq!(devanagari_to_bengali("केटे फलार काथा छिलो"), "কেটে ফলার কাথা ছিলো");
    }

    #[test]
    fn keeps_shared_and_other_characters() {
        assert_eq!(devanagari_to_bengali("আমি বাংলায় কথা বলি।"), "আমি বাংলায় কথা বলি।");
        assert_eq!(devanagari_to_bengali("Hello, 2024!"), "Hello, 2024!");
        assert_eq!(devanagari_to_bengali("वन"), "বন");
    }

    #[test]
    fn replaces_unassigned_bengali_code_points() {
        // Real Whisper output: "঵েবসাইটে" with U+09B5, which fonts can't draw.
        assert_eq!(devanagari_to_bengali("\u{09B5}েবসাইটে"), "বেবসাইটে");
        assert!(devanagari_to_bengali("\u{09B1}\u{09B4}").chars().all(is_assigned_bengali));
    }

    #[test]
    fn only_bengali_script_languages_are_repaired() {
        assert!(uses_bengali_script("bn"));
        assert!(uses_bengali_script("as"));
        assert!(!uses_bengali_script("hi"));
    }
}
