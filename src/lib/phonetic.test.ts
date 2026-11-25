import { describe, expect, it } from "vitest";
import { banglaSymbol, banglaWords, buildVocab, spell, wordBefore } from "./phonetic";
import { BANGLA_WORDS } from "./banglaWords";

const nfc = (s: string) => s.normalize("NFC");
const top = (roman: string, before = "", vocab = buildVocab([], BANGLA_WORDS)) => spell(roman, before, vocab)[0];

describe("phonetic rules (no dictionary)", () => {
  const plain = (roman: string, before = "") => spell(roman, before)[0];
  it.each([
    ["ami", "আমি"],
    ["amra", "আমরা"],
    ["apni", "আপনি"],
    ["kintu", "কিন্তু"],
    ["ebong", "এবং"],
    ["bangladesh", "বাংলাদেশ"],
    ["hoyeche", "হয়েছে"],
    ["rongin", "রঙিন"],
    ["golpo", "গল্প"],
    ["korte", "করতে"],
    ["korchi", "করছি"],
    ["shikkha", "শিক্ষা"],
    ["biggan", "বিজ্ঞান"],
    ["sundor", "সুন্দর"],
    ["bondhu", "বন্ধু"],
    ["rasta", "রাস্তা"],
    ["uttor", "উত্তর"],
    ["proshno", "প্রশ্ন"],
    ["du:kho", "দুঃখ"],
    ["pa^c", "পাঁচ"],
    ["onek", "অনেক"],
    ["amader", "আমাদের"],
    ["tOmar", "তোমার"],
    ["TOTO", "টোটো"],
    ["k`k", "কক"],
  ])("%s → %s", (roman, bangla) => {
    expect(plain(roman)).toBe(nfc(bangla));
  });

  it("continues a word that is already partly written", () => {
    expect(plain("te", "কর")).toBe("তে");
    expect(plain("tu", "কিন")).toBe(nfc("্তু"));
  });

  it("offers the other likely spellings", () => {
    const all = spell("korte");
    expect(all).toContain("করতে");
    expect(all).toContain(nfc("কর্তে"));
    expect(spell("ami").length).toBeGreaterThan(1);
  });

  it("ignores characters it doesn't know", () => {
    expect(spell("")).toEqual([]);
    expect(spell("123")).toEqual([]);
  });
});

describe("dictionary ranking", () => {
  it.each([
    ["jonno", "জন্য"],
    ["bhalo", "ভালো"],
    ["chai", "চাই"],
    ["taka", "টাকা"],
    ["bari", "বাড়ি"],
    ["dhonnobad", "ধন্যবাদ"],
    ["utsob", "উৎসব"],
    ["porikkha", "পরীক্ষা"],
    ["tomar", "তোমার"],
    ["jodi", "যদি"],
    ["ekta", "একটা"],
    ["o", "ও"],
  ])("%s → %s", (roman, bangla) => {
    expect(top(roman)).toBe(nfc(bangla));
  });

  it("prefers words already used in the project", () => {
    const vocab = buildVocab(["আমরা সারাদিনের জন্য রিজার্ভ করে নিচ্ছি", "রিজার্ভ"]);
    expect(top("rijarbh", "", vocab)).toBe(nfc("রিজার্ভ"));
  });

  it("ranks a known stem with a new ending above unknown spellings", () => {
    expect(top("takay")).toBe(nfc("টাকায়"));
  });
});

describe("speed", () => {
  it("re-spells a word fast enough for every keystroke", () => {
    const vocab = buildVocab([], BANGLA_WORDS);
    let worst = 0;
    for (const w of ["khub", "sundor", "jonno", "bishshobiddaloy", "aamaaderbangladesherjonnoshikkhaprotishthan"]) {
      for (let i = 1; i <= w.length; i++) {
        const t = performance.now();
        spell(w.slice(0, i), "", vocab);
        worst = Math.max(worst, performance.now() - t);
      }
    }
    expect(worst).toBeLessThan(50);
  });
});

describe("vocabulary", () => {
  it("collects Bangla words without digits, punctuation or joiners", () => {
    expect(banglaWords("আমি ৩০০০ টাকা দিলাম। Hello বাংলা‌")).toEqual(["আমি", "টাকা", "দিলাম", "বাংলা"].map(nfc));
  });

  it("finds the part of the word before the caret", () => {
    expect(wordBefore("আমি কর", 6)).toBe("কর");
    expect(wordBefore("আমি ", 4)).toBe("");
    expect(wordBefore("দাম ৩০", 6)).toBe("");
  });
});

describe("symbols", () => {
  it("types Bangla digits, dari and taka sign", () => {
    expect(banglaSymbol("3", "")).toBe("৩");
    expect(banglaSymbol(".", "আমি")).toBe("।");
    expect(banglaSymbol(".", "৩")).toBeNull();
    expect(banglaSymbol("$", "")).toBe("৳");
    expect(banglaSymbol(",", "")).toBeNull();
  });
});
