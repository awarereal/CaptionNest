import { describe, expect, it } from "vitest";
import { CAPTION_FONTS, DEFAULT_STYLE, PRESETS, cssFont, fontById, fontMissing, matchingPreset, nearestWeight, normalizeStyle, setUserFonts } from "./captionStyle";
import { isRtlText, wrapLines } from "./renderCaption";

describe("caption style", () => {
  it("repairs missing or invalid values", () => {
    const s = normalizeStyle({ font: "nope", size: 99, color: "red", position: "left", boxOpacity: -5, weight: 900 });
    expect(s.font).toBe(DEFAULT_STYLE.font);
    expect(s.size).toBe(11);
    expect(s.color).toBe(DEFAULT_STYLE.color);
    expect(s.position).toBe("bottom");
    expect(s.boxOpacity).toBe(0);
    expect(s.weight).toBe(800);
    expect(normalizeStyle(undefined)).toEqual(DEFAULT_STYLE);
  });

  it("keeps every preset valid and recognisable", () => {
    for (const p of PRESETS) {
      expect(normalizeStyle(p.style)).toEqual(p.style);
      expect(matchingPreset(p.style)).toBe(p.id);
    }
    expect(matchingPreset({ ...DEFAULT_STYLE, size: 7.7 })).toBeNull();
  });

  it("uses only weights a font has", () => {
    const tiro = CAPTION_FONTS.find((f) => f.id === "tiro-bangla")!;
    expect(nearestWeight(tiro, 800)).toBe(400);
    expect(cssFont({ ...DEFAULT_STYLE, font: "tiro-bangla", weight: 800 }, 20)).toMatch(/^400 20\.00px "Tiro Bangla"/);
  });
});

describe("caption layout", () => {
  const measure = (s: string) => Array.from(s).length * 10;

  it("keeps line breaks and wraps long lines at spaces", () => {
    expect(wrapLines(measure, "short\nএকটি লম্বা লাইন যা ভাঙতে হবে", 120)).toEqual(["short", "একটি লম্বা", "লাইন যা", "ভাঙতে হবে"]);
  });

  it("breaks text without spaces by character", () => {
    expect(wrapLines(measure, "这是一个很长的句子没有空格", 50)).toEqual(["这是一个很", "长的句子没", "有空格"]);
  });

  it("detects right-to-left text from the first letter", () => {
    expect(isRtlText("  مرحبًا بكم")).toBe(true);
    expect(isRtlText("12 hello")).toBe(false);
    expect(isRtlText("আমি")).toBe(false);
  });
});

describe("added fonts", () => {
  const mine = { id: "user-0123456789ab", name: "Kalpurush", family: '"CN User user-0123456789ab"', weights: [400, 700], note: "", user: true };

  it("keeps an added font's id even before the font is loaded", () => {
    setUserFonts([]);
    expect(normalizeStyle({ font: mine.id }).font).toBe(mine.id);
    expect(normalizeStyle({ font: "user-nothex" }).font).toBe(DEFAULT_STYLE.font);
    expect(fontMissing(mine.id)).toBe(true);
    expect(fontById(mine.id).id).toBe(CAPTION_FONTS[0].id);
  });

  it("draws with the added font once it's loaded", () => {
    setUserFonts([mine]);
    expect(fontMissing(mine.id)).toBe(false);
    expect(cssFont({ ...DEFAULT_STYLE, font: mine.id, weight: 600 }, 10)).toMatch(/^700 10\.00px "CN User user-0123456789ab", "Anek Bangla/);
    setUserFonts([]);
  });
});
