// Languages Whisper can recognize, with honest quality guidance.
//
// Tiers follow the Whisper paper's FLEURS word error rates for the large models
// (https://arxiv.org/abs/2212.04356, Appendix D) and CaptionNest's own Bangla benchmark:
//   good    – usually accurate on clear speech
//   fair    – usable, expect regular corrections
//   limited – a rough draft; review every caption
// No language is guaranteed. Accents, noise, music and overlapping voices all lower accuracy.

export type Tier = "good" | "fair" | "limited";

export interface Language {
  code: string;
  name: string;
  native?: string;
  tier: Tier;
  rtl?: boolean;
  note?: string;
}

const L = (code: string, name: string, native: string | undefined, tier: Tier, extra: Partial<Language> = {}): Language => ({
  code,
  name,
  native,
  tier,
  ...extra,
});

export const SPOKEN_LANGUAGES: Language[] = [
  L("en", "English", undefined, "good"),
  L("bn", "Bangla", "বাংলা", "limited", {
    note: "Needs the Accurate model to write Bengali script. Expect spelling errors; review every caption.",
  }),
  L("hi", "Hindi", "हिन्दी", "fair", { note: "Mixed Hindi–English speech may switch between scripts." }),
  L("ar", "Arabic", "العربية", "fair", {
    rtl: true,
    note: "Modern Standard Arabic works best; regional dialects are recognized less reliably.",
  }),
  L("ur", "Urdu", "اردو", "fair", { rtl: true }),
  L("es", "Spanish", "Español", "good"),
  L("fr", "French", "Français", "good"),
  L("de", "German", "Deutsch", "good"),
  L("it", "Italian", "Italiano", "good"),
  L("pt", "Portuguese", "Português", "good"),
  L("ru", "Russian", "Русский", "good"),
  L("ja", "Japanese", "日本語", "good"),
  L("ko", "Korean", "한국어", "good"),
  L("zh", "Chinese (Mandarin)", "中文", "good"),
  L("yue", "Cantonese", "粵語", "fair"),
  L("tr", "Turkish", "Türkçe", "good"),
  L("nl", "Dutch", "Nederlands", "good"),
  L("pl", "Polish", "Polski", "good"),
  L("sv", "Swedish", "Svenska", "good"),
  L("id", "Indonesian", "Bahasa Indonesia", "good"),
  L("ms", "Malay", "Bahasa Melayu", "good"),
  L("vi", "Vietnamese", "Tiếng Việt", "good"),
  L("uk", "Ukrainian", "Українська", "good"),
  L("ca", "Catalan", "Català", "good"),
  L("fi", "Finnish", "Suomi", "good"),
  L("cs", "Czech", "Čeština", "good"),
  L("da", "Danish", "Dansk", "good"),
  L("no", "Norwegian", "Norsk", "good"),
  L("ro", "Romanian", "Română", "good"),
  L("el", "Greek", "Ελληνικά", "good"),
  L("hu", "Hungarian", "Magyar", "good"),
  L("gl", "Galician", "Galego", "good"),
  L("sk", "Slovak", "Slovenčina", "fair"),
  L("he", "Hebrew", "עברית", "fair", { rtl: true }),
  L("fa", "Persian", "فارسی", "fair", { rtl: true }),
  L("th", "Thai", "ไทย", "fair"),
  L("ta", "Tamil", "தமிழ்", "fair"),
  L("bg", "Bulgarian", "Български", "fair"),
  L("hr", "Croatian", "Hrvatski", "fair"),
  L("sr", "Serbian", "Српски", "fair"),
  L("sl", "Slovenian", "Slovenščina", "fair"),
  L("lt", "Lithuanian", "Lietuvių", "fair"),
  L("lv", "Latvian", "Latviešu", "fair"),
  L("et", "Estonian", "Eesti", "fair"),
  L("az", "Azerbaijani", "Azərbaycanca", "fair"),
  L("mk", "Macedonian", "Македонски", "fair"),
  L("bs", "Bosnian", "Bosanski", "fair"),
  L("kk", "Kazakh", "Қазақ", "fair"),
  L("af", "Afrikaans", "Afrikaans", "fair"),
  L("tl", "Tagalog", "Tagalog", "fair"),
  L("cy", "Welsh", "Cymraeg", "fair"),
  L("is", "Icelandic", "Íslenska", "fair"),
  L("be", "Belarusian", "Беларуская", "fair"),
  L("sq", "Albanian", "Shqip", "fair"),
  L("hy", "Armenian", "Հայերեն", "limited"),
  L("ka", "Georgian", "ქართული", "limited"),
  L("mr", "Marathi", "मराठी", "limited"),
  L("ne", "Nepali", "नेपाली", "limited"),
  L("te", "Telugu", "తెలుగు", "limited"),
  L("kn", "Kannada", "ಕನ್ನಡ", "limited"),
  L("ml", "Malayalam", "മലയാളം", "limited"),
  L("gu", "Gujarati", "ગુજરાતી", "limited"),
  L("pa", "Punjabi", "ਪੰਜਾਬੀ", "limited"),
  L("si", "Sinhala", "සිංහල", "limited"),
  L("as", "Assamese", "অসমীয়া", "limited"),
  L("sw", "Swahili", "Kiswahili", "limited"),
  L("am", "Amharic", "አማርኛ", "limited"),
  L("my", "Burmese", "မြန်မာ", "limited"),
  L("km", "Khmer", "ខ្មែរ", "limited"),
  L("lo", "Lao", "ລາວ", "limited"),
  L("mn", "Mongolian", "Монгол", "limited"),
  L("uz", "Uzbek", "Oʻzbek", "limited"),
  L("tg", "Tajik", "Тоҷикӣ", "limited"),
  L("tk", "Turkmen", "Türkmen", "limited"),
  L("ps", "Pashto", "پښتو", "limited", { rtl: true }),
  L("sd", "Sindhi", "سنڌي", "limited", { rtl: true }),
  L("yi", "Yiddish", "ייִדיש", "limited", { rtl: true }),
  L("so", "Somali", "Soomaali", "limited"),
  L("yo", "Yoruba", "Yorùbá", "limited"),
  L("ha", "Hausa", "Hausa", "limited"),
  L("sn", "Shona", "chiShona", "limited"),
  L("ln", "Lingala", "Lingála", "limited"),
  L("mg", "Malagasy", "Malagasy", "limited"),
  L("ht", "Haitian Creole", "Kreyòl ayisyen", "limited"),
  L("jw", "Javanese", "Basa Jawa", "limited"),
  L("su", "Sundanese", "Basa Sunda", "limited"),
  L("eu", "Basque", "Euskara", "fair"),
  L("mt", "Maltese", "Malti", "limited"),
  L("lb", "Luxembourgish", "Lëtzebuergesch", "limited"),
  L("fo", "Faroese", "Føroyskt", "limited"),
  L("oc", "Occitan", "Occitan", "limited"),
  L("br", "Breton", "Brezhoneg", "limited"),
  L("la", "Latin", "Latina", "limited"),
  L("sa", "Sanskrit", "संस्कृतम्", "limited"),
  L("mi", "Māori", "Te Reo Māori", "limited"),
  L("haw", "Hawaiian", "ʻŌlelo Hawaiʻi", "limited"),
  L("bo", "Tibetan", "བོད་སྐད", "limited"),
  L("tt", "Tatar", "Татар", "limited"),
  L("ba", "Bashkir", "Башҡорт", "limited"),
  L("nn", "Norwegian Nynorsk", "Nynorsk", "fair"),
];

const BY_CODE = new Map(SPOKEN_LANGUAGES.map((l) => [l.code, l]));

/** Extra names for Argos-only codes. */
const ARGOS_NAMES: Record<string, Partial<Language>> = {
  pb: { name: "Portuguese (Brazil)", native: "Português (Brasil)" },
  zt: { name: "Chinese (Traditional)", native: "中文（繁體）" },
  nb: { name: "Norwegian Bokmål", native: "Norsk bokmål" },
  eo: { name: "Esperanto", native: "Esperanto" },
  ga: { name: "Irish", native: "Gaeilge" },
  ky: { name: "Kyrgyz", native: "Кыргызча" },
};

export function language(code: string | null | undefined): Language | undefined {
  if (!code) return undefined;
  const known = BY_CODE.get(code);
  if (known) return known;
  const extra = ARGOS_NAMES[code];
  return extra ? { code, tier: "fair", name: extra.name ?? code, native: extra.native } : undefined;
}

export function languageName(code: string | null | undefined, fallback = "Unknown language"): string {
  if (!code || code === "und") return fallback;
  return language(code)?.name ?? code.toUpperCase();
}

export const isRtl = (code: string | null | undefined) => !!language(code)?.rtl;

export const TIER_LABEL: Record<Tier, string> = {
  good: "Usually accurate",
  fair: "Expect corrections",
  limited: "Rough draft",
};

/** Whisper codes → the codes Argos packages use. */
export function argosCode(code: string): string {
  if (code === "no" || code === "nn") return "nb";
  if (code === "jw") return "jv";
  return code;
}
