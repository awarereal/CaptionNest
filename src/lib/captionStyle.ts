// How captions look on the video: fonts, presets and the style saved with a project.
// The same style drives the live preview and videos exported with burned-in captions.
// SRT files carry only text and timing, so CapCut applies its own style to those.

export type CaptionPosition = "top" | "middle" | "bottom";

export interface CaptionStyle {
  font: string;
  weight: number;
  /** Text size as a percentage of the frame's shorter side. */
  size: number;
  color: string;
  /** Outline thickness as a percentage of the text size (0 = none). */
  outline: number;
  outlineColor: string;
  shadow: boolean;
  box: boolean;
  boxColor: string;
  /** Background opacity, 0-100. */
  boxOpacity: number;
  position: CaptionPosition;
  /** Distance from the top or bottom edge, as a percentage of the frame height. */
  margin: number;
  uppercase: boolean;
}

export interface CaptionFont {
  id: string;
  name: string;
  /** CSS family list for the font's own faces (Latin + Bengali). */
  family: string;
  weights: number[];
  note: string;
  /** A font the user added (see userFonts.ts). */
  user?: boolean;
  /** Whether it has Bengali letters (added fonts only; null when unknown). */
  bangla?: boolean | null;
}

/** Scripts a caption font doesn't cover fall back to these, in order. */
const FALLBACK = '"Anek Bangla Variable", "Anek Devanagari Variable", "Readex Pro Variable", sans-serif';

export const CAPTION_FONTS: CaptionFont[] = [
  { id: "anek", name: "Anek", family: '"Anek Latin Variable", "Anek Bangla Variable"', weights: [400, 500, 600, 700, 800], note: "Modern and compact" },
  { id: "hind-siliguri", name: "Hind Siliguri", family: '"Hind Siliguri"', weights: [400, 500, 600, 700], note: "Plain and very easy to read" },
  { id: "noto-sans", name: "Noto Sans", family: '"Noto Sans Variable", "Noto Sans Bengali Variable"', weights: [400, 500, 600, 700, 800], note: "Broadcast classic" },
  { id: "baloo-da-2", name: "Baloo Da 2", family: '"Baloo Da 2 Variable"', weights: [400, 500, 600, 700, 800], note: "Rounded and friendly" },
  { id: "atma", name: "Atma", family: '"Atma"', weights: [400, 500, 600, 700], note: "Hand-drawn and playful" },
  { id: "tiro-bangla", name: "Tiro Bangla", family: '"Tiro Bangla"', weights: [400], note: "Serif, for documentaries" },
];

export const WEIGHT_NAMES: Record<number, string> = {
  100: "Thin",
  200: "Extra light",
  300: "Light",
  400: "Regular",
  500: "Medium",
  600: "Semibold",
  700: "Bold",
  800: "Heavy",
  900: "Black",
};

export interface Preset {
  id: string;
  name: string;
  style: CaptionStyle;
}

const base: CaptionStyle = {
  font: "anek",
  weight: 600,
  size: 5.4,
  color: "#ffffff",
  outline: 7,
  outlineColor: "#0b0907",
  shadow: true,
  box: false,
  boxColor: "#0b0907",
  boxOpacity: 72,
  position: "bottom",
  margin: 8,
  uppercase: false,
};

export const PRESETS: Preset[] = [
  { id: "clean", name: "Clean", style: base },
  { id: "broadcast", name: "Broadcast", style: { ...base, font: "noto-sans", weight: 500, size: 4.8, outline: 0, shadow: false, box: true, margin: 7 } },
  { id: "bold", name: "Bold", style: { ...base, weight: 800, size: 6.6, outline: 13, outlineColor: "#000000", margin: 20, uppercase: true } },
  { id: "highlight", name: "Highlight", style: { ...base, font: "baloo-da-2", weight: 700, size: 5.6, color: "#14110e", outline: 0, shadow: false, box: true, boxColor: "#f2bd4b", boxOpacity: 100 } },
  { id: "soft", name: "Soft", style: { ...base, font: "hind-siliguri", weight: 600, size: 5.2, color: "#fff4dc", outline: 0 } },
  { id: "story", name: "Story", style: { ...base, font: "tiro-bangla", weight: 400, size: 5.2, outline: 0, margin: 9 } },
];

export const DEFAULT_STYLE: CaptionStyle = base;

let userFonts: CaptionFont[] = [];

/** Fonts the user added, as loaded by userFonts.ts. */
export function setUserFonts(fonts: CaptionFont[]) {
  userFonts = fonts;
}

/** Built-in caption fonts, then the ones the user added. */
export function allFonts(): CaptionFont[] {
  return [...CAPTION_FONTS, ...userFonts];
}

/** Ids of added fonts look like `user-0123456789ab` (from the family name). */
export const isUserFontId = (id: string) => /^user-[0-9a-f]{12}$/.test(id);

/** A style that names an added font which isn't on this computer (anymore). */
export function fontMissing(id: string): boolean {
  return isUserFontId(id) && !userFonts.some((f) => f.id === id);
}

/** The font to draw with; a missing added font falls back to the default. */
export function fontById(id: string): CaptionFont {
  return CAPTION_FONTS.find((f) => f.id === id) ?? userFonts.find((f) => f.id === id) ?? CAPTION_FONTS[0];
}

/** The closest weight the font actually has. */
export function nearestWeight(font: CaptionFont, weight: number): number {
  return font.weights.reduce((best, w) => (Math.abs(w - weight) < Math.abs(best - weight) ? w : best), font.weights[0]);
}

/** CSS `font` shorthand for drawing at `px` pixels. */
export function cssFont(style: CaptionStyle, px: number): string {
  const font = fontById(style.font);
  return `${nearestWeight(font, style.weight)} ${px.toFixed(2)}px ${font.family}, ${FALLBACK}`;
}

const HEX = /^#[0-9a-f]{6}$/i;
const clamp = (v: unknown, lo: number, hi: number, d: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d);
const color = (v: unknown, d: string) => (typeof v === "string" && HEX.test(v) ? v.toLowerCase() : d);
const bool = (v: unknown, d: boolean) => (typeof v === "boolean" ? v : d);

export const SIZE_RANGE = [3, 11] as const;
export const MARGIN_RANGE = [2, 40] as const;

/** Accepts anything (an older project, hand-edited JSON) and returns a valid style. */
export function normalizeStyle(raw: unknown): CaptionStyle {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_STYLE;
  // An added font's id is kept even before (or without) the font being loaded, so a
  // project opened on another computer shows its font again once it's added there.
  const font = typeof r.font === "string" && (CAPTION_FONTS.some((f) => f.id === r.font) || isUserFontId(r.font)) ? r.font : d.font;
  return {
    font,
    weight: nearestWeight(fontById(font), clamp(r.weight, 300, 900, d.weight)),
    size: clamp(r.size, SIZE_RANGE[0], SIZE_RANGE[1], d.size),
    color: color(r.color, d.color),
    outline: clamp(r.outline, 0, 20, d.outline),
    outlineColor: color(r.outlineColor, d.outlineColor),
    shadow: bool(r.shadow, d.shadow),
    box: bool(r.box, d.box),
    boxColor: color(r.boxColor, d.boxColor),
    boxOpacity: clamp(r.boxOpacity, 0, 100, d.boxOpacity),
    position: r.position === "top" || r.position === "middle" || r.position === "bottom" ? r.position : d.position,
    margin: clamp(r.margin, MARGIN_RANGE[0], MARGIN_RANGE[1], d.margin),
    uppercase: bool(r.uppercase, d.uppercase),
  };
}

/** The preset this style matches exactly, if any. */
export function matchingPreset(style: CaptionStyle): string | null {
  const keys = Object.keys(style) as (keyof CaptionStyle)[];
  return PRESETS.find((p) => keys.every((k) => p.style[k] === style[k]))?.id ?? null;
}

/** Makes sure the faces needed to draw `text` are loaded (they load on demand). */
export async function ensureFonts(style: CaptionStyle, text: string): Promise<void> {
  if (typeof document === "undefined" || !document.fonts) return;
  try {
    await document.fonts.load(cssFont(style, 40), text.trim() || "Aa আমি");
  } catch {
    // Drawing falls back to the next available font.
  }
}

const LAST_KEY = "captionnest.captionStyle";

/** The style used last, for new projects. */
export function lastStyle(): CaptionStyle {
  try {
    const raw = localStorage.getItem(LAST_KEY);
    return raw ? normalizeStyle(JSON.parse(raw)) : DEFAULT_STYLE;
  } catch {
    return DEFAULT_STYLE;
  }
}

export function rememberStyle(style: CaptionStyle) {
  try {
    localStorage.setItem(LAST_KEY, JSON.stringify(style));
  } catch {
    // Not critical.
  }
}
