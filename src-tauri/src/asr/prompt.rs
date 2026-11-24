//! Initial prompts that keep Whisper writing normal punctuation and capitalization.
//!
//! With timestamped decoding, large-v3 models often drop punctuation and capitals
//! entirely ("welcome to caption nest this short video"). A short, natural, punctuated
//! sentence in the target language as the initial prompt restores both; CaptionNest's
//! tests showed it also improves sentence punctuation for Bangla. The prompt must be in
//! the language (and script) being written, so it is chosen after detection.

pub fn initial_prompt(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "en" => "Punctuation matters: commas, periods, and capital letters.",
        "bn" => "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?",
        "hi" => "नमस्ते, आप कैसे हैं? आज मौसम अच्छा है।",
        "ar" => "مرحبًا، كيف حالك؟ الطقس جميل اليوم.",
        "ur" => "السلام علیکم، آپ کیسے ہیں؟ آج موسم اچھا ہے۔",
        "es" => "Hola, ¿cómo estás? Hoy hace buen tiempo.",
        "fr" => "Bonjour, comment allez-vous ? Il fait beau aujourd'hui.",
        "de" => "Hallo, wie geht es dir? Heute ist schönes Wetter.",
        "pt" => "Olá, como vai? Hoje o tempo está bom.",
        "it" => "Ciao, come stai? Oggi fa bel tempo.",
        "nl" => "Hallo, hoe gaat het? Vandaag is het mooi weer.",
        "ru" => "Привет, как дела? Сегодня хорошая погода.",
        "uk" => "Привіт, як справи? Сьогодні гарна погода.",
        "pl" => "Cześć, jak się masz? Dziś jest ładna pogoda.",
        "tr" => "Merhaba, nasılsın? Bugün hava güzel.",
        "id" | "ms" => "Halo, apa kabar? Hari ini cuacanya bagus.",
        "vi" => "Xin chào, bạn khỏe không? Hôm nay trời đẹp.",
        "ja" => "こんにちは。今日はいい天気ですね。",
        "zh" => "你好，今天天气很好。我们出去走走吧。",
        "ko" => "안녕하세요. 오늘 날씨가 좋네요.",
        "fa" => "سلام، حال شما چطور است؟ امروز هوا خوب است.",
        "ta" => "வணக்கம், எப்படி இருக்கிறீர்கள்? இன்று வானிலை நன்றாக உள்ளது.",
        _ => return None,
    })
}

/// Removes the initial prompt when Whisper outputs it as if it were speech (this can
/// happen on unclear audio). To never delete real speech, a chunk is only cleared when
/// it consists of nothing but whole prompt sentences; prompts are chosen to be phrases
/// people don't naturally say.
pub fn strip_echo(words: Vec<super::Word>, prompt: &str) -> Vec<super::Word> {
    let norm = |s: &str| -> String { s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect() };
    let sentences: Vec<String> = prompt
        .split(['.', '?', '!', '।', '؟', '。', '۔'])
        .map(norm)
        .filter(|s| s.chars().count() >= 6)
        .collect();
    if sentences.is_empty() {
        return words;
    }
    let mut start = 0;
    'outer: while start < words.len() {
        let mut acc = String::new();
        for end in start..words.len().min(start + 24) {
            acc.push_str(&norm(&words[end].text));
            if sentences.iter().any(|s| *s == acc) {
                start = end + 1;
                continue 'outer;
            }
            if !sentences.iter().any(|s| s.starts_with(&acc)) {
                break;
            }
        }
        break;
    }
    if start > 0 && start == words.len() { Vec::new() } else { words }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::Word;

    fn w(t: &str) -> Word {
        Word { confidence: 1.0, start_ms: 0, end_ms: 1, text: format!(" {t}") }
    }

    #[test]
    fn clears_a_chunk_that_only_repeats_the_prompt() {
        let p = initial_prompt("en").unwrap();
        let words = vec![w("Punctuation"), w("matters:"), w("commas,"), w("periods,"), w("and"), w("capital"), w("letters.")];
        assert!(strip_echo(words, p).is_empty());
    }

    #[test]
    fn never_removes_real_speech() {
        let p = initial_prompt("en").unwrap();
        let mixed = vec![w("Punctuation"), w("matters:"), w("commas,"), w("periods,"), w("and"), w("capital"), w("letters."), w("Real"), w("speech.")];
        assert_eq!(strip_echo(mixed, p).len(), 9);
        let normal = vec![w("Hello"), w("everyone,"), w("welcome"), w("back.")];
        assert_eq!(strip_echo(normal, p).len(), 4);
    }

    #[test]
    fn core_languages_have_punctuated_prompts() {
        for l in ["en", "bn", "hi", "ar"] {
            let p = initial_prompt(l).unwrap();
            assert!(p.chars().any(|c| ".?।؟,،".contains(c)), "{l}");
        }
        assert!(initial_prompt("xx").is_none());
    }
}
