//! Initial prompts that keep Whisper writing normal punctuation and capitalization.
//!
//! With timestamped decoding, large-v3 models often drop punctuation and capitals
//! entirely ("welcome to caption nest this short video"). A short, natural, punctuated
//! sentence in the target language as the initial prompt restores both; CaptionNest's
//! tests showed it also improves sentence punctuation for Bangla. The prompt must be in
//! the language (and script) being written, so it is chosen after detection.

pub fn initial_prompt(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "en" => "Hello, and welcome. This is how we speak.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_languages_have_punctuated_prompts() {
        for l in ["en", "bn", "hi", "ar"] {
            let p = initial_prompt(l).unwrap();
            assert!(p.chars().any(|c| ".?।؟,،".contains(c)), "{l}");
        }
        assert!(initial_prompt("xx").is_none());
    }
}
