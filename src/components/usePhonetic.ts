// Bangla phonetic typing inside a caption's text box: Latin letters become Bangla as you
// type ("ami" → আমি). While a word is being typed its letters are kept, so each new key
// re-spells the whole word; ↑/↓ pick another spelling, Esc keeps the English letters,
// and any other key (space, punctuation, arrows, a click) finishes the word.
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { create } from "zustand";
import { PHONETIC_KEY, banglaSymbol, buildVocab, spell, wordBefore, type Vocab } from "../lib/phonetic";
import { BANGLA_WORDS } from "../lib/banglaWords";
import { useStore } from "../state/store";
import type { Track } from "../lib/types";

const KEY = "captionnest.banglaTyping";

function readSetting(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

/** Whether Bangla captions turn Latin letters into Bangla (on unless switched off). */
export const useBanglaTyping = create<{ on: boolean; toggle: () => void }>((set, get) => ({
  on: readSetting(),
  toggle() {
    const on = !get().on;
    try {
      localStorage.setItem(KEY, on ? "on" : "off");
    } catch {
      // Not critical.
    }
    set({ on });
  },
}));

let cache: { tracks: Track[]; vocab: Vocab } | null = null;

/** Words in the project's Bangla captions plus common words, rebuilt when captions change. */
function projectVocab(): Vocab {
  const tracks = useStore.getState().history.present;
  if (cache?.tracks !== tracks) {
    const texts = tracks.filter((t) => t.language === "bn").flatMap((t) => t.cues.map((c) => c.text));
    cache = { tracks, vocab: buildVocab(texts, BANGLA_WORDS) };
  }
  return cache.vocab;
}

export interface Composition {
  /** Where the word starts in the text. */
  start: number;
  roman: string;
  /** The Bangla currently in the text for this word. */
  shown: string;
  options: string[];
  index: number;
  /** Bangla letters of the same word before the caret when the word began. */
  before: string;
  vocab: Vocab;
}

export function usePhonetic(ref: RefObject<HTMLTextAreaElement | null>, enabled: boolean, setValue: (value: string) => void) {
  const [comp, setCompState] = useState<Composition | null>(null);
  const compRef = useRef<Composition | null>(null);
  const caret = useRef<number | null>(null);
  const setComp = (c: Composition | null) => {
    compRef.current = c;
    setCompState(c);
  };

  // The text box is controlled: put the caret back after React writes the new value.
  useLayoutEffect(() => {
    const el = ref.current;
    if (el && caret.current !== null) {
      el.setSelectionRange(caret.current, caret.current);
      caret.current = null;
    }
  });

  const write = (el: HTMLTextAreaElement, from: number, to: number, insert: string) => {
    const value = el.value;
    const next = value.slice(0, from) + insert + value.slice(to);
    const at = from + insert.length;
    if (next === value) {
      el.setSelectionRange(at, at);
      return;
    }
    caret.current = at;
    setValue(next);
  };

  /** The word is still where it was typed and the caret is right after it. */
  const intact = (el: HTMLTextAreaElement, c: Composition) =>
    el.selectionStart === el.selectionEnd && el.selectionStart === c.start + c.shown.length && el.value.slice(c.start, c.start + c.shown.length) === c.shown;

  const finish = () => {
    if (compRef.current) setComp(null);
  };

  /** Handles a key; returns true when it was used for Bangla typing. */
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!enabled) return false;
    const el = e.currentTarget;
    // A system input method (e.g. the OS Bangla keyboard) is composing: leave it alone.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return false;
    let c = compRef.current;
    if (c && !intact(el, c)) {
      setComp(null);
      c = null;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) {
      finish();
      return false;
    }
    const key = e.key;

    if (PHONETIC_KEY.test(key) || (key === ":" && c)) {
      const start = c ? c.start : el.selectionStart;
      const end = c ? c.start + c.shown.length : el.selectionEnd;
      const before = c ? c.before : wordBefore(el.value, start);
      const vocab = c ? c.vocab : projectVocab();
      const roman = (c?.roman ?? "") + key;
      const options = spell(roman, before, vocab);
      if (options.length === 0) return false;
      e.preventDefault();
      write(el, start, end, options[0]);
      setComp({ start, roman, shown: options[0], options, index: 0, before, vocab });
      return true;
    }

    if (c) {
      if (key === "Backspace") {
        e.preventDefault();
        const roman = c.roman.slice(0, -1);
        const options = roman ? spell(roman, c.before, c.vocab) : [];
        write(el, c.start, c.start + c.shown.length, options[0] ?? "");
        setComp(options.length > 0 ? { ...c, roman, shown: options[0], options, index: 0 } : null);
        return true;
      }
      if ((key === "ArrowDown" || key === "ArrowUp") && c.options.length > 1) {
        e.preventDefault();
        const index = (c.index + (key === "ArrowDown" ? 1 : -1) + c.options.length) % c.options.length;
        write(el, c.start, c.start + c.shown.length, c.options[index]);
        setComp({ ...c, index, shown: c.options[index] });
        return true;
      }
      if (key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        write(el, c.start, c.start + c.shown.length, c.roman);
        setComp(null);
        return true;
      }
      if (key === "Enter" && !e.shiftKey) {
        // Enter accepts the word, as in other input methods.
        e.preventDefault();
        setComp(null);
        return true;
      }
      if (key === "Shift" || key === "CapsLock") return false;
      setComp(null);
    }

    if (key.length === 1) {
      const symbol = banglaSymbol(key, el.value.slice(0, el.selectionStart));
      if (symbol) {
        e.preventDefault();
        write(el, el.selectionStart, el.selectionEnd, symbol);
        return true;
      }
    }
    return false;
  };

  /** Picks spelling `i` (from the suggestion strip) and finishes the word. */
  const choose = (i: number) => {
    const el = ref.current;
    const c = compRef.current;
    if (!el || !c || !intact(el, c) || !c.options[i]) return;
    write(el, c.start, c.start + c.shown.length, c.options[i]);
    setComp(null);
    el.focus();
  };

  return { comp: enabled ? comp : null, onKeyDown, finish, choose };
}
