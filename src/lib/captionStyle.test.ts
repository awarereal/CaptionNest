import { describe, expect, it } from "vitest";
import { CAPTION_FONTS, DEFAULT_STYLE, PRESETS, cssFont, matchingPreset, nearestWeight, normalizeStyle } from "./captionStyle";
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
