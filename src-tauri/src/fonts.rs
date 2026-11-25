//! Fonts the user adds for captions.
//!
//! Each font file is copied into the app's data folder (so the original can be moved or
//! deleted) and listed in `fonts.json`. Files of one family (Regular, Bold…) become one
//! font with several weights. For TrueType/OpenType files the family name, weight,
//! italic flag, variable weight range and Bangla coverage are read from the font's own
//! tables; for WOFF/WOFF2 they are guessed from the file name. The interface loads each
//! file as bytes through `font_file`, because the web view's content policy allows no
//! other font source.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult, ErrorCode};

const INDEX: &str = "fonts.json";
const MAX_BYTES: u64 = 40 * 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserFont {
    /// `user-` + 12 hex digits from the family name, so the same font added on another
    /// computer gets the same id and projects find it.
    pub id: String,
    pub name: String,
    pub faces: Vec<Face>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    /// File name inside the fonts folder (`<hash>.<ext>`).
    pub file: String,
    pub weight: u16,
    /// Upper end of the weight range for variable fonts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_max: Option<u16>,
    pub italic: bool,
    /// Whether the font has Bengali letters (None when it can't be read).
    pub bangla: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub fonts: Vec<UserFont>,
    /// Ids of the fonts that received new files.
    pub added: Vec<String>,
    pub errors: Vec<ImportError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportError {
    pub file: String,
    pub message: String,
}

pub fn list(dir: &Path) -> Vec<UserFont> {
    std::fs::read(dir.join(INDEX)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, fonts: &[UserFont]) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, &serde_json::to_vec_pretty(fonts)?)?;
    tmp.as_file().sync_all()?;
    crate::project::persist(tmp, &dir.join(INDEX))
}

pub fn import(dir: &Path, paths: &[PathBuf]) -> AppResult<ImportResult> {
    let _guard = LOCK.lock().unwrap();
    let mut fonts = list(dir);
    let mut added = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match import_one(dir, &mut fonts, path) {
            Ok(id) => {
                if !added.contains(&id) {
                    added.push(id);
                }
            }
            Err(e) => errors.push(ImportError { file, message: e.message }),
        }
    }
    fonts.sort_by_key(|f| f.name.to_lowercase());
    save(dir, &fonts)?;
    Ok(ImportResult { fonts, added, errors })
}

fn import_one(dir: &Path, fonts: &mut Vec<UserFont>, path: &Path) -> AppResult<String> {
    let not_font = || AppError::new(ErrorCode::FontInvalid, "This file isn't a font CaptionNest can use. Choose a .ttf, .otf, .woff or .woff2 file.");
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BYTES {
        return Err(AppError::new(ErrorCode::FontInvalid, "This font file is too large (over 40 MB)."));
    }
    let bytes = std::fs::read(path)?;
    let ext = match bytes.get(..4) {
        Some([0, 1, 0, 0]) | Some(b"true") => "ttf",
        Some(b"OTTO") => "otf",
        Some(b"wOFF") => "woff",
        Some(b"wOF2") => "woff2",
        Some(b"ttcf") => {
            return Err(AppError::new(ErrorCode::FontInvalid, "Font collections (.ttc) can't be used. Choose a single .ttf or .otf file."));
        }
        _ => return Err(not_font()),
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let meta = match ext {
        "ttf" | "otf" => read_sfnt(&bytes).ok_or_else(not_font)?,
        _ => Meta::from_file_name(&stem),
    };
    let hash = hex::encode(Sha256::digest(&bytes));
    let file = format!("{}.{ext}", &hash[..16]);
    let id = font_id(&meta.family);

    if fonts.iter().any(|f| f.faces.iter().any(|x| x.file == file)) {
        return Ok(fonts.iter().find(|f| f.faces.iter().any(|x| x.file == file)).map(|f| f.id.clone()).unwrap_or(id));
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(&file), &bytes)?;
    let face = Face { file, weight: meta.weight, weight_max: meta.weight_max, italic: meta.italic, bangla: meta.bangla };
    match fonts.iter_mut().find(|f| f.id == id) {
        Some(font) => {
            // A second file with the same weight and style replaces the older one.
            if let Some(old) = font.faces.iter().position(|x| x.weight == face.weight && x.italic == face.italic && x.weight_max == face.weight_max) {
                let gone = font.faces.remove(old);
                let _ = std::fs::remove_file(dir.join(gone.file));
            }
            font.faces.push(face);
            font.faces.sort_by_key(|f| (f.italic, f.weight));
        }
        None => fonts.push(UserFont { id: id.clone(), name: meta.family, faces: vec![face] }),
    }
    Ok(id)
}

pub fn remove(dir: &Path, id: &str) -> AppResult<Vec<UserFont>> {
    let _guard = LOCK.lock().unwrap();
    let mut fonts = list(dir);
    if let Some(i) = fonts.iter().position(|f| f.id == id) {
        let font = fonts.remove(i);
        for face in font.faces {
            let _ = std::fs::remove_file(dir.join(face.file));
        }
        save(dir, &fonts)?;
    }
    Ok(fonts)
}

/// Reads one font file for the interface. Only names this module created are accepted.
pub fn read_file(dir: &Path, file: &str) -> AppResult<Vec<u8>> {
    let valid = file.split_once('.').is_some_and(|(hash, ext)| {
        hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) && matches!(ext, "ttf" | "otf" | "woff" | "woff2")
    });
    if !valid {
        return Err(AppError::internal("invalid font file name"));
    }
    std::fs::read(dir.join(file)).map_err(|e| AppError::new(ErrorCode::FileNotFound, "A font you added is missing. Add it again in Caption style.").with_detail(e.to_string()))
}

fn font_id(family: &str) -> String {
    let hash = hex::encode(Sha256::digest(family.trim().to_lowercase().as_bytes()));
    format!("user-{}", &hash[..12])
}

// ---------- reading the font's own tables ----------

#[derive(Debug, PartialEq)]
struct Meta {
    family: String,
    weight: u16,
    weight_max: Option<u16>,
    italic: bool,
    bangla: Option<bool>,
}

impl Meta {
    fn from_file_name(stem: &str) -> Self {
        let (family, style, number) = split_style(stem);
        let weight = number.unwrap_or_else(|| weight_from_name(&style));
        Meta { family, weight, weight_max: None, italic: is_italic_name(&style), bangla: None }
    }
}

/// Words that describe a style, longest first so "extrabold" wins over "bold".
const STYLE_WORDS: [&str; 19] = [
    "extralight", "ultralight", "extrabold", "ultrabold", "semibold", "demibold", "hairline", "regular", "oblique", "italic", "normal", "medium",
    "light", "black", "heavy", "thin", "bold", "book", "variable",
];
/// Character subsets in web-font file names (e.g. "hind-siliguri-bengali-700-normal").
const SUBSETS: [&str; 9] = ["latin", "ext", "bengali", "devanagari", "cyrillic", "greek", "vietnamese", "arabic", "math"];

/// A name part made only of style words, like "SemiBold" or "BoldItalic".
fn is_style_word(token: &str) -> bool {
    let mut s = token.to_lowercase();
    while !s.is_empty() {
        match STYLE_WORDS.iter().find(|w| s.starts_with(*w)) {
            Some(w) => s = s[w.len()..].to_string(),
            None => return false,
        }
    }
    !token.is_empty()
}

/// Splits a file name into family, style words and a numeric weight:
/// "Hind_Siliguri-SemiBold" → ("Hind Siliguri", "semibold", None),
/// "hind-siliguri-bengali-700-normal" → ("Hind Siliguri", "normal", Some(700)).
fn split_style(stem: &str) -> (String, String, Option<u16>) {
    let mut tokens: Vec<&str> = stem.split(['-', '_', ' ']).filter(|t| !t.is_empty()).collect();
    let mut style = String::new();
    let mut number = None;
    while tokens.len() > 1 {
        let last = tokens[tokens.len() - 1];
        let lower = last.to_lowercase();
        if let Ok(n) = lower.parse::<u16>() {
            if (100..=900).contains(&n) && n % 100 == 0 {
                number.get_or_insert(n);
            }
        } else if is_style_word(last) {
            style = format!("{lower}{style}");
        } else if !SUBSETS.contains(&lower.as_str()) {
            break;
        }
        tokens.pop();
    }
    // Lower-case file names ("hind-siliguri") read better capitalized.
    let family = tokens
        .iter()
        .map(|t| if t.chars().all(|c| !c.is_uppercase()) { capitalize(t) } else { t.to_string() })
        .collect::<Vec<_>>()
        .join(" ");
    (if family.is_empty() { "My font".into() } else { family }, style, number)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn weight_from_name(style: &str) -> u16 {
    let s = style.to_lowercase().replace([' ', '-', '_'], "");
    for (word, w) in [
        ("thin", 100),
        ("hairline", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("light", 300),
        ("medium", 500),
        ("black", 900),
        ("heavy", 900),
        ("bold", 700),
    ] {
        if s.contains(word) {
            return w;
        }
    }
    400
}

fn is_italic_name(style: &str) -> bool {
    let s = style.to_lowercase();
    s.contains("italic") || s.contains("oblique")
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn table<'a>(b: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = u16_at(b, 4)? as usize;
    for i in 0..count {
        let rec = 12 + i * 16;
        if b.get(rec..rec + 4)? == tag {
            let off = u32_at(b, rec + 8)? as usize;
            let len = u32_at(b, rec + 12)? as usize;
            return b.get(off..off.checked_add(len)?);
        }
    }
    None
}

fn read_sfnt(b: &[u8]) -> Option<Meta> {
    let name = table(b, b"name")?;
    let family = name_string(name, 16).or_else(|| name_string(name, 1))?;
    let subfamily = name_string(name, 17).or_else(|| name_string(name, 2)).unwrap_or_default();
    let os2 = table(b, b"OS/2");
    let weight = os2.and_then(|t| u16_at(t, 4)).filter(|w| (1..=1000).contains(w)).unwrap_or_else(|| weight_from_name(&subfamily));
    let italic = os2.and_then(|t| u16_at(t, 62)).map(|sel| sel & 1 == 1).unwrap_or(false) || is_italic_name(&subfamily);
    let (weight, weight_max) = match table(b, b"fvar").and_then(weight_axis) {
        Some((lo, hi)) if hi > lo => (lo, Some(hi)),
        _ => (round_weight(weight), None),
    };
    let bangla = table(b, b"cmap").map(|t| has_char(t, 0x0995));
    Some(Meta { family: family.trim().to_string(), weight, weight_max, italic, bangla })
}

fn round_weight(w: u16) -> u16 {
    (((w as f32) / 100.0).round() as u16 * 100).clamp(100, 900)
}

/// A name-table string, preferring Windows English, then any Windows, then Mac Roman.
fn name_string(t: &[u8], id: u16) -> Option<String> {
    let count = u16_at(t, 2)? as usize;
    let storage = u16_at(t, 4)? as usize;
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, encoding, lang, name_id) = (u16_at(t, r)?, u16_at(t, r + 2)?, u16_at(t, r + 4)?, u16_at(t, r + 6)?);
        if name_id != id {
            continue;
        }
        let (len, off) = (u16_at(t, r + 8)? as usize, u16_at(t, r + 10)? as usize);
        let Some(raw) = t.get(storage + off..storage + off + len) else { continue };
        let (rank, text) = match (platform, encoding) {
            (3, 0 | 1 | 10) | (0, _) => {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                (if platform == 3 && lang == 0x409 { 0 } else { 1 }, String::from_utf16_lossy(&units))
            }
            (1, 0) => (2, raw.iter().map(|&c| if c.is_ascii() { c as char } else { '?' }).collect()),
            _ => continue,
        };
        if !text.trim().is_empty() && best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, s)| s)
}

/// The `wght` axis range of a variable font.
fn weight_axis(t: &[u8]) -> Option<(u16, u16)> {
    let axes_offset = u16_at(t, 4)? as usize;
    let count = u16_at(t, 8)? as usize;
    let size = u16_at(t, 10)? as usize;
    for i in 0..count {
        let a = axes_offset + i * size;
        if t.get(a..a + 4)? == b"wght" {
            let fixed = |at: usize| u32_at(t, at).map(|v| ((v as i32) >> 16) as i64);
            let lo = fixed(a + 4)?.clamp(1, 1000) as u16;
            let hi = fixed(a + 12)?.clamp(1, 1000) as u16;
            return Some((lo, hi));
        }
    }
    None
}

/// Whether the character map has `ch` (formats 4 and 12, the common Unicode ones).
fn has_char(t: &[u8], ch: u32) -> bool {
    let Some(count) = u16_at(t, 2) else { return false };
    for i in 0..count as usize {
        let r = 4 + i * 8;
        let Some(off) = u32_at(t, r + 4) else { continue };
        let sub = &t[(off as usize).min(t.len())..];
        let found = match u16_at(sub, 0) {
            Some(4) => format4_has(sub, ch),
            Some(12) => format12_has(sub, ch),
            _ => None,
        };
        if found == Some(true) {
            return true;
        }
    }
    false
}

fn format4_has(s: &[u8], ch: u32) -> Option<bool> {
    if ch > 0xFFFF {
        return Some(false);
    }
    let segs = u16_at(s, 6)? as usize / 2;
    for i in 0..segs {
        let end = u16_at(s, 14 + i * 2)? as u32;
        let start = u16_at(s, 16 + segs * 2 + i * 2)? as u32;
        if (start..=end).contains(&ch) {
            // Treat a mapped range as covered; fonts rarely map Bengali to glyph 0.
            return Some(true);
        }
    }
    Some(false)
}

fn format12_has(s: &[u8], ch: u32) -> Option<bool> {
    let groups = u32_at(s, 12)? as usize;
    for i in 0..groups.min(100_000) {
        let g = 16 + i * 12;
        let (start, end) = (u32_at(s, g)?, u32_at(s, g + 4)?);
        if (start..=end).contains(&ch) {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal TrueType file with name, OS/2 and cmap tables.
    fn tiny_font(family: &str, weight: u16, italic: bool, bangla: bool) -> Vec<u8> {
        let utf16: Vec<u8> = family.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let mut name = Vec::new();
        name.extend_from_slice(&[0, 0, 0, 1, 0, 18]); // format, count, storage offset
        name.extend_from_slice(&[0, 3, 0, 1, 0x04, 0x09, 0, 1]); // Windows, Unicode BMP, en-US, family
        name.extend_from_slice(&(utf16.len() as u16).to_be_bytes());
        name.extend_from_slice(&[0, 0]);
        name.extend_from_slice(&utf16);
        let mut os2 = vec![0u8; 64];
        os2[4..6].copy_from_slice(&weight.to_be_bytes());
        os2[62..64].copy_from_slice(&(italic as u16).to_be_bytes());
        // cmap with one format 4 subtable covering one range (+ the required 0xFFFF end).
        let (lo, hi): (u16, u16) = if bangla { (0x0980, 0x09FF) } else { (0x0020, 0x007E) };
        let mut sub = Vec::new();
        sub.extend_from_slice(&[0, 4, 0, 32, 0, 0, 0, 4, 0, 4, 0, 1, 0, 0]);
        sub.extend_from_slice(&hi.to_be_bytes());
        sub.extend_from_slice(&[0xFF, 0xFF, 0, 0]);
        sub.extend_from_slice(&lo.to_be_bytes());
        sub.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 1, 0, 0, 0, 0]);
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12];
        cmap.extend_from_slice(&sub);

        let tables: [(&[u8; 4], &Vec<u8>); 3] = [(b"OS/2", &os2), (b"cmap", &cmap), (b"name", &name)];
        let mut out = vec![0, 1, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0];
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            offset += data.len();
        }
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn reads_family_weight_style_and_bangla_coverage() {
        let m = read_sfnt(&tiny_font("Kalpurush", 400, false, true)).unwrap();
        assert_eq!(m, Meta { family: "Kalpurush".into(), weight: 400, weight_max: None, italic: false, bangla: Some(true) });
        let m = read_sfnt(&tiny_font("Mono Sans", 680, true, false)).unwrap();
        assert_eq!((m.weight, m.italic, m.bangla), (700, true, Some(false)));
    }

    #[test]
    fn file_names_give_family_and_weight() {
        assert_eq!(Meta::from_file_name("Hind_Siliguri-SemiBold").family, "Hind Siliguri");
        assert_eq!(Meta::from_file_name("Hind_Siliguri-SemiBold").weight, 600);
        assert_eq!(Meta::from_file_name("Roboto-BoldItalic").italic, true);
        assert_eq!(Meta::from_file_name("SolaimanLipi").family, "SolaimanLipi");
        assert_eq!(Meta::from_file_name("Noto-Sans").family, "Noto Sans");
        let web = Meta::from_file_name("hind-siliguri-bengali-700-normal");
        assert_eq!((web.family.as_str(), web.weight, web.italic), ("Hind Siliguri", 700, false));
        assert_eq!(Meta::from_file_name("Times-New-Roman").family, "Times New Roman");
        assert_eq!(Meta::from_file_name("Kalpurush-ExtraBold").weight, 800);
    }

    #[test]
    fn faces_of_one_family_are_grouped_and_duplicates_skipped() {
        let src = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let regular = src.path().join("a.ttf");
        let bold = src.path().join("b.ttf");
        let junk = src.path().join("notes.ttf");
        std::fs::write(&regular, tiny_font("Kalpurush", 400, false, true)).unwrap();
        std::fs::write(&bold, tiny_font("Kalpurush", 700, false, true)).unwrap();
        std::fs::write(&junk, b"hello, not a font").unwrap();

        let r = import(dir.path(), &[regular.clone(), bold, junk]).unwrap();
        assert_eq!(r.fonts.len(), 1);
        assert_eq!(r.fonts[0].faces.iter().map(|f| f.weight).collect::<Vec<_>>(), vec![400, 700]);
        assert_eq!(r.added, vec![font_id("Kalpurush")]);
        assert_eq!(r.errors.len(), 1);
        assert_eq!(r.errors[0].file, "notes.ttf");

        let again = import(dir.path(), &[regular]).unwrap();
        assert_eq!(again.fonts[0].faces.len(), 2, "the same file isn't added twice");
        let file = &again.fonts[0].faces[0].file;
        assert!(read_file(dir.path(), file).unwrap().starts_with(&[0, 1, 0, 0]));
        assert!(read_file(dir.path(), "../settings.json").is_err());

        let left = remove(dir.path(), &font_id("Kalpurush")).unwrap();
        assert!(left.is_empty());
        assert!(!dir.path().join(file).exists());
    }

    #[test]
    fn ids_are_stable_across_computers() {
        assert_eq!(font_id("Kalpurush"), font_id(" kalpurush "));
        assert!(font_id("Kalpurush").starts_with("user-") && font_id("Kalpurush").len() == 17);
    }
}
