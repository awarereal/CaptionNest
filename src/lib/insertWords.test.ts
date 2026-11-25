import { describe, expect, it } from "vitest";
import { insertWords } from "./insertWords";

describe("dictated words", () => {
  it("go in at the cursor and keep the rest", () => {
    const text = "আমি বাড়ি যাব";
    expect(insertWords(text, { start: 4, end: 4, text }, "আজ")).toEqual({ text: "আমি আজ বাড়ি যাব", caret: 6 });
  });

  it("replace selected text only", () => {
    const text = "আমি বাড়ি যাব";
    expect(insertWords(text, { start: 4, end: 9, text }, "অফিসে").text).toBe("আমি অফিসে যাব");
  });

  it("fill an empty caption", () => {
    expect(insertWords("", { start: 0, end: 0, text: "" }, "নমস্কার।")).toEqual({ text: "নমস্কার।", caret: 8 });
  });

  it("don't add a space before punctuation", () => {
    const text = "আমি";
    expect(insertWords(text, { start: 3, end: 3, text }, "।").text).toBe("আমি।");
    expect(insertWords("ভালো।", { start: 0, end: 0, text: "ভালো।" }, "খুব").text).toBe("খুব ভালো।");
  });

  it("never replace text that changed while listening", () => {
    const before = "আমি বাড়ি যাব";
    // Text after the cursor was edited: insert at the cursor without replacing.
    expect(insertWords("আমি বাসায় যাব", { start: 4, end: 9, text: before }, "আজ").text).toBe("আমি আজ বাসায় যাব");
    // Text before the cursor changed: add at the end.
    expect(insertWords("তুমি বাড়ি যাব", { start: 4, end: 9, text: before }, "আজ").text).toBe("তুমি বাড়ি যাব আজ");
  });
});
