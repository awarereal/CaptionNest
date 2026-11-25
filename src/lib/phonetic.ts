// Bangla phonetic typing: Latin letters typed the way Bangla sounds ("ami" → "আমি"),
// following the Avro phonetic conventions most Bangla typists already know
// (o / O, i / I, t / T, sh / S, ng for ং, rri for ঋ, ^ for ঁ, ` to stop a conjunct).
//
// Phonetic spelling is ambiguous: "korte" can be করতে or কর্তে, "jonno" is usually জন্য,
// "taka" is টাকা. So instead of one fixed answer the converter explores the likely
// spellings, gives each less common choice a small cost, and ranks the results by the
// words this project already contains plus a list of common Bangla words. The best one
// is typed; the others are offered as choices.

/** Known words with how often they appear, and every prefix of them (for the search). */
export interface Vocab {
  words: Map<string, number>;
  prefixes: Set<string>;
}

type Prev = "none" | "C" | "V" | "W";

type Tok =
  | { k: "c"; opts: [string, number][]; t?: true }
  | { k: "v"; opts: [string, string, number][]; a?: true; e?: true }
  | { k: "o" }
  | { k: "y" }
  | { k: "w" }
  | { k: "Z" }
  | { k: "ng" }
  | { k: "mark"; ch: string }
  | { k: "sep" };

// Letters with a nukta, written decomposed (the form Unicode normalization keeps).
const RRA = "ড়"; // ড়
const RHA = "ঢ়"; // ঢ়
const YYA = "য়"; // য়
const HASANTA = "্";

const C = (...opts: [string, number][]): Tok => ({ k: "c", opts });
const V = (...opts: [string, string, number][]): Tok => ({ k: "v", opts });
const I_SHORT = V(["ই", "ি", 0], ["ঈ", "ী", 0.5]);
const I_LONG = V(["ঈ", "ী", 0], ["ই", "ি", 0.6]);
const U_SHORT = V(["উ", "ু", 0], ["ঊ", "ূ", 0.6]);
const U_LONG = V(["ঊ", "ূ", 0], ["উ", "ু", 0.6]);

/** Roman letter groups. Case matters where Avro gives it a meaning (t/T, o/O, sh/S…). */
const TABLE = new Map<string, Tok>([
  ["k", C(["ক", 0])],
  ["kh", C(["খ", 0])],
  ["g", C(["গ", 0])],
  ["gh", C(["ঘ", 0])],
  ["Ng", C(["ঙ", 0])],
  ["c", C(["চ", 0], ["ছ", 0.6])],
  ["ch", C(["ছ", 0], ["চ", 0.4])],
  ["j", C(["জ", 0], ["য", 0.5])],
  ["J", C(["জ", 0])],
  ["jh", C(["ঝ", 0])],
  ["NG", C(["ঞ", 0])],
  ["T", C(["ট", 0])],
  ["Th", C(["ঠ", 0])],
  ["D", C(["ড", 0])],
  ["Dh", C(["ঢ", 0])],
  ["N", C(["ণ", 0])],
  ["t", { k: "c", opts: [["ত", 0], ["ট", 0.7]], t: true }],
  ["th", C(["থ", 0], ["ঠ", 0.8])],
  ["d", C(["দ", 0], ["ড", 0.7])],
  ["dh", C(["ধ", 0], ["ঢ", 0.9])],
  ["n", C(["ন", 0], ["ণ", 0.6])],
  ["p", C(["প", 0])],
  ["ph", C(["ফ", 0])],
  ["f", C(["ফ", 0])],
  ["b", C(["ব", 0])],
  ["bh", C(["ভ", 0])],
  ["v", C(["ভ", 0])],
  ["m", C(["ম", 0])],
  ["z", C(["য", 0], ["জ", 0.6])],
  ["r", C(["র", 0], [RRA, 0.6])],
  ["R", C([RRA, 0])],
  ["Rh", C([RHA, 0])],
  ["l", C(["ল", 0])],
  ["sh", C(["শ", 0], ["ষ", 0.5], ["স", 0.7])],
  ["S", C(["ষ", 0])],
  ["Sh", C(["ষ", 0])],
  ["s", C(["স", 0], ["শ", 0.4])],
  ["h", C(["হ", 0])],
  ["q", C(["ক", 0])],
  ["x", C(["ক্স", 0], ["ক্ষ", 0.8])],
  ["kkh", C(["ক্ষ", 0], ["ক্খ", 1.5])],
  ["ngg", C(["ঙ্গ", 0])],
  ["gg", C(["জ্ঞ", 0], ["গ্গ", 0.5])],
  ["t``", C(["ৎ", 0])],
  ["a", { k: "v", opts: [["আ", "া", 0]], a: true }],
  ["i", I_SHORT],
  ["I", I_LONG],
  ["ee", I_LONG],
  ["u", U_SHORT],
  ["U", U_LONG],
  ["oo", U_LONG],
  ["e", { k: "v", opts: [["এ", "ে", 0]], e: true }],
  ["O", V(["ও", "ো", 0])],
  ["OI", V(["ঐ", "ৈ", 0])],
  ["OU", V(["ঔ", "ৌ", 0])],
  ["rri", V(["ঋ", "ৃ", 0])],
  ["o", { k: "o" }],
  ["y", { k: "y" }],
  ["Y", { k: "y" }],
  ["w", { k: "w" }],
  ["Z", { k: "Z" }],
  ["ng", { k: "ng" }],
  ["^", { k: "mark", ch: "ঁ" }],
  [":", { k: "mark", ch: "ঃ" }],
  ["`", { k: "sep" }],
]);

/** Characters that belong to a phonetic word while it is being typed. */
export const PHONETIC_KEY = /^[A-Za-z`^]$/;

/** Splits roman input into letter groups, longest match first. */
function tokenize(roman: string): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  while (i < roman.length) {
    let tok: Tok | undefined;
    let len = 0;
    for (let n = Math.min(4, roman.length - i); n >= 1 && !tok; n--) {
      const s = roman.slice(i, i + n);
      tok = TABLE.get(s) ?? TABLE.get(s.toLowerCase());
      if (tok) len = n;
    }
    if (!tok) {
      i++;
      continue;
    }
    out.push(tok);
    i += len;
  }
  return out;
}

// Consonants that commonly take ro-fola (প্র, ক্র, ত্র…). Not ম/ন/ল: "amra" is আমরা.
const RO_FOLA = new Set([..."কখগঘটডতথদধপফবভশষস"]);
// Real doubled conjuncts (ক্ক, ত্ত, ন্ন…). Other doubles are usually a য-ফলা sound.
const REAL_DOUBLE = new Set([..."কগচজটডণতদনপবমল"]);
// Clusters that are almost always written as a conjunct when typed without a vowel.
const COMMON_JOIN = new Set(
  "কত কস কষ গধ গন ঙক ঙখ ঙগ ঙঘ চছ জঞ ঞচ ঞছ ঞজ ণট ণঠ ণড তথ তন তম দধ দভ দম নত নথ নদ নধ নট নড পত বদ বধ মপ মব মভ লপ শচ শন ষক ষট ষঠ ষণ ষপ ষম সক সট সত সথ সপ সফ".split(" "),
);
// Real conjuncts that are less likely than two separate letters ("apni" is আপনি).
const VALID_JOIN = new Set(
  "কট কল কম কব গল গম গব জব তব দব দগ দঘ ধন ধব ধম নস নম নব পন পল পট পস বজ বল মন মফ মল লক লগ লট লড লব লম শল শম শব সন সল সখ সম সব হন হম হল হব".split(" "),
);

type Join = "join" | "sep" | "yaf";

function pairOptions(a: string, b: string): [Join, number][] {
  if (b === "র") return a === "র" ? [["sep", 0]] : RO_FOLA.has(a) ? [["join", 0], ["sep", 0.9]] : [["sep", 0]];
  if (a === "র") return [["sep", 0], ["join", 0.7]];
  if (a === b) return REAL_DOUBLE.has(a) ? [["join", 0], ["yaf", 0.7], ["sep", 1.5]] : [["sep", 0], ["yaf", 0.7]];
  if (COMMON_JOIN.has(a + b)) return [["join", 0], ["sep", 1.2]];
  if (VALID_JOIN.has(a + b)) return [["sep", 0], ["join", 1]];
  return [["sep", 0]];
}

const CONSONANT = /[ক-হ]/;
/** A consonant that can carry a hasanta into the next one (not ড়, ঢ়, য়, ৎ). */
const joinable = (s: string) => CONSONANT.test(s.slice(-1)) && s !== "ৎ";

interface State {
  out: string;
  prev: Prev;
  /** The consonant the next one may join to ("" when none may). */
  join: string;
  /** Just wrote a য-ফলা (an "a" right after it is often silent: ব্যবসা). */
  yaf: boolean;
  pen: number;
}

const vowelish = (t: Tok | undefined) => !!t && (t.k === "v" || t.k === "o");

function step(s: State, tok: Tok, next: Tok | undefined): State[] {
  const res: State[] = [];
  const push = (out: string, prev: Prev, join: string, pen: number, yaf = false) => res.push({ out: s.out + out, prev, join, yaf, pen: s.pen + pen });

  switch (tok.k) {
    case "sep":
      return [{ ...s, join: "" }];
    case "c": {
      const opts = [...tok.opts];
      if (tok.t && !vowelish(next) && next?.k !== "y" && next?.k !== "w") opts.push(["ৎ", next ? 1 : 0.6]);
      for (const [ch, p] of opts) {
        const keep = joinable(ch) ? ch.slice(-1) : "";
        if (s.prev === "C" && s.join && joinable(ch) && !ch.startsWith(HASANTA)) {
          for (const [how, q] of pairOptions(s.join, ch[0])) {
            if (how === "join") push(HASANTA + ch, "C", keep, p + q);
            else if (how === "sep") push(ch, "C", keep, p + q);
            else push(HASANTA + "য", "C", "", p + q, true);
          }
        } else {
          push(ch, "C", keep, p);
        }
      }
      return res;
    }
    case "v":
      for (const [ind, kar, p] of tok.opts) {
        if (s.prev === "C") {
          push(kar, "V", "", p);
          if (tok.a && s.yaf) push("", "V", "", p + 0.6);
        } else if (s.prev === "W" && (tok.a || tok.e)) {
          push(YYA + kar, "V", "", p);
        } else {
          push(ind, "V", "", p);
        }
      }
      return res;
    case "o":
      if (s.prev === "C") {
        push("", "V", "", 0);
        push("ো", "V", "", next ? 0.6 : 0.35);
      } else if (s.prev === "none") {
        push("অ", "V", "", 0);
        push("ও", "V", "", 0.6);
      } else {
        push("ও", "V", "", 0);
        push("অ", "V", "", 0.8);
      }
      return res;
    case "y":
      if (s.prev === "C") push(HASANTA + "য", "C", "", 0, true);
      else push(YYA, "C", "", 0);
      return res;
    case "w":
      if (s.prev === "C") push(HASANTA + "ব", "C", "", 0);
      else push("ও", "W", "", 0);
      return res;
    case "Z":
      push(s.prev === "C" ? HASANTA + "য" : "য", "C", "", 0, s.prev === "C");
      return res;
    case "ng":
      // ঙ before a vowel (রঙিন, বাঙালি), ং otherwise (বাংলা, এবং).
      if (vowelish(next)) {
        push("ঙ", "C", "ঙ", 0);
        push("ং", "V", "", 1);
      } else {
        push("ং", "V", "", 0);
        push("ঙ", "C", "ঙ", 0.8);
      }
      return res;
    case "mark":
      push(tok.ch, "V", "", 0);
      return res;
  }
}

/** Where a new word starts, given the Bangla letters right before the caret. */
function startState(before: string): State {
  const last = before.slice(-1);
  const base: State = { out: "", prev: "none", join: "", yaf: false, pen: 0 };
  if (!last) return base;
  if (CONSONANT.test(last)) return { ...base, prev: "C", join: last };
  if (last === "়" || last === "ৎ") return { ...base, prev: "C" };
  if (last === HASANTA) return base;
  return { ...base, prev: "V" };
}

const norm = (s: string) => s.normalize("NFC");
const BEAM = 80;

/**
 * Bangla spellings for a roman word, best first. `before` is the part of the word already
 * written before the caret (so "te" typed after "কর" becomes "তে", not "এ").
 */
export function spell(roman: string, before = "", vocab?: Vocab, limit = 6): string[] {
  const toks = tokenize(roman);
  if (toks.length === 0) return [];
  let beam: State[] = [startState(before)];
  const prefixHit = (s: State) => (vocab?.prefixes.has(norm(before + s.out)) ? 0 : 1);
  toks.forEach((tok, i) => {
    const next = toks[i + 1];
    const seen = new Map<string, State>();
    for (const s of beam) {
      for (const n of step(s, tok, next)) {
        const key = `${n.out}|${n.prev}|${n.join}|${n.yaf}`;
        const old = seen.get(key);
        if (!old || n.pen < old.pen) seen.set(key, n);
      }
    }
    beam = [...seen.values()].sort((a, b) => prefixHit(a) - prefixHit(b) || a.pen - b.pen).slice(0, BEAM);
  });

  const scored = new Map<string, number>();
  for (const s of beam) {
    const out = norm(s.out);
    const score = scoreWord(norm(before + s.out), s.pen, vocab);
    if (!scored.has(out) || score > scored.get(out)!) scored.set(out, score);
  }
  return [...scored.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
    .map(([w]) => w);
}

function scoreWord(word: string, pen: number, vocab?: Vocab): number {
  const base = -10 * pen;
  if (!vocab) return base;
  const freq = vocab.words.get(word);
  if (freq) return 100 + 5 * Math.log2(1 + freq) + base;
  // A known word at the start (a stem with an ending, like টাকা + য়).
  for (let n = word.length - 1; n >= 2; n--) {
    if (vocab.words.has(word.slice(0, n))) return 40 * (n / word.length) + base;
  }
  return base;
}

// ---------- vocabulary ----------

const WORD = /[ঀ-৿‌‍]+/g;
const DIGIT = /[০-৯]/;

/** Bangla words in `text` (no digits, no joiners), normalized. */
export function banglaWords(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(WORD)) {
    const w = norm(m[0].replace(/[‌‍]/g, ""));
    if (w && !DIGIT.test(w)) out.push(w);
  }
  return out;
}

/** Builds a vocabulary from caption texts, with `seed` words counted `seedWeight` times. */
export function buildVocab(texts: Iterable<string>, seed: readonly string[] = [], seedWeight = 3): Vocab {
  const words = new Map<string, number>();
  for (const w of seed) words.set(norm(w), seedWeight);
  for (const t of texts) for (const w of banglaWords(t)) words.set(w, (words.get(w) ?? 0) + 1);
  const prefixes = new Set<string>();
  for (const w of words.keys()) for (let n = 1; n <= w.length; n++) prefixes.add(w.slice(0, n));
  return { words, prefixes };
}

/** The Bangla letters of the current word immediately before `at`. */
export function wordBefore(text: string, at: number): string {
  const m = /[ঀ-৥ৰ-৿‌‍]+$/.exec(text.slice(0, at));
  return m ? m[0] : "";
}

const BN_DIGITS = "০১২৩৪৫৬৭৮৯";

/** What a single non-letter key types in Bangla mode, or null to type it unchanged. */
export function banglaSymbol(key: string, before: string): string | null {
  if (key >= "0" && key <= "9") return BN_DIGITS[key.charCodeAt(0) - 48];
  // A full stop ends a Bangla sentence with a dari, except inside a number (৩.৫).
  if (key === ".") return /[0-9০-৯]$/.test(before) ? null : "।";
  if (key === "$") return "৳";
  return null;
}
