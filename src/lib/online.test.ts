import { describe, expect, it } from "vitest";
import { onlinePlan, syncWithDeepgram } from "./online";
import type { Settings } from "./types";

const base = { onlineRecognition: true, sarvamKeyHint: null, onlineEngine: "both" } as unknown as Settings;
const withSarvam = { ...base, sarvamKeyHint: "sk_ab…9lnQ" } as Settings;

describe("onlinePlan", () => {
  it("uses Sarvam's words with Deepgram's timing when both keys are saved", () => {
    expect(onlinePlan(withSarvam, true, "bn", false)).toBe("sarvam+deepgram");
  });

  it("uses whichever service has a key", () => {
    expect(onlinePlan(withSarvam, false, "bn", false)).toBe("sarvam");
    expect(onlinePlan(base, true, "bn", false)).toBe("deepgram");
    expect(onlinePlan(base, false, "bn", true)).toBeNull();
  });

  it("follows the engine setting", () => {
    expect(onlinePlan({ ...withSarvam, onlineEngine: "sarvam" }, true, "bn", true)).toBe("sarvam");
    expect(onlinePlan({ ...withSarvam, onlineEngine: "deepgram" }, true, "bn", true)).toBe("deepgram");
  });

  it("needs the language for Deepgram: detected by an offline model, or chosen", () => {
    expect(onlinePlan(base, true, "auto", false)).toBeNull();
    expect(onlinePlan(base, true, "auto", true)).toBe("deepgram");
    // Sarvam detects the language itself.
    expect(onlinePlan(withSarvam, true, "auto", false)).toBe("sarvam");
  });

  it("falls back to the other service for languages one doesn't know", () => {
    expect(onlinePlan(withSarvam, true, "ta", true)).toBe("sarvam");
    expect(onlinePlan(withSarvam, true, "es", true)).toBe("deepgram");
    expect(onlinePlan(base, true, "ta", true)).toBeNull();
  });

  it("is off when online recognition is switched off", () => {
    expect(onlinePlan({ ...withSarvam, onlineRecognition: false }, true, "bn", true)).toBeNull();
  });
});

describe("syncWithDeepgram", () => {
  it("listens with Deepgram when its key is saved, unless Sarvam only is chosen", () => {
    expect(syncWithDeepgram(base, true, "bn")).toBe(true);
    expect(syncWithDeepgram(base, false, "bn")).toBe(false);
    expect(syncWithDeepgram({ ...base, onlineEngine: "sarvam" }, true, "bn")).toBe(false);
    expect(syncWithDeepgram({ ...base, onlineRecognition: false }, true, "bn")).toBe(false);
    expect(syncWithDeepgram(base, true, "ta")).toBe(false);
  });
});
