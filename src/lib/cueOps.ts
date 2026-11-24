// Pure caption editing operations. Every function returns a new array.
import type { Cue } from "./types";

export const MIN_CUE_MS = 200;

export const newId = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `c-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

export function sortCues(cues: Cue[]): Cue[] {
  return [...cues].sort((a, b) => a.startMs - b.startMs || a.endMs - b.endMs);
}

export function updateCue(cues: Cue[], id: string, patch: Partial<Omit<Cue, "id">>): Cue[] {
  return cues.map((c) => (c.id === id ? { ...c, ...patch } : c));
}

/** Changes timing and keeps the list in start order. */
export function retimeCue(cues: Cue[], id: string, startMs: number, endMs: number): Cue[] {
  return sortCues(updateCue(cues, id, { startMs: Math.max(0, startMs), endMs: Math.max(0, endMs) }));
}

export function deleteCue(cues: Cue[], id: string): Cue[] {
  return cues.filter((c) => c.id !== id);
}

/**
 * Adds an empty cue at `atMs`, sized to fit the gap before the next cue
 * (2 s by default, at least MIN_CUE_MS). Returns the new list and the new cue id.
 */
export function addCue(cues: Cue[], atMs: number, durationMs = 2000, maxMs = Infinity): { cues: Cue[]; id: string } {
  const sorted = sortCues(cues);
  const start = Math.max(0, Math.round(atMs));
  const next = sorted.find((c) => c.startMs > start);
  const limit = Math.min(next ? next.startMs : Infinity, maxMs);
  const end = Math.max(start + MIN_CUE_MS, Math.min(start + durationMs, limit));
  const cue: Cue = { id: newId(), startMs: start, endMs: end, text: "" };
  return { cues: sortCues([...sorted, cue]), id: cue.id };
}

/**
 * Splits a cue in two. Text is split at `caret` (a character index) when given,
 * otherwise at the word boundary nearest the middle. Time is split at `atMs` when it
 * falls inside the cue, otherwise in proportion to the text split.
 */
export function splitCue(cues: Cue[], id: string, opts: { caret?: number; atMs?: number } = {}): Cue[] {
  const i = cues.findIndex((c) => c.id === id);
  if (i < 0) return cues;
  const cue = cues[i];
  const flat = cue.text;
  let at = opts.caret ?? nearestBoundary(flat, Math.floor(flat.length / 2));
  at = Math.max(0, Math.min(flat.length, at));
  const left = flat.slice(0, at).trim();
  const right = flat.slice(at).trim();
  const duration = cue.endMs - cue.startMs;
  let mid: number;
  if (opts.atMs !== undefined && opts.atMs > cue.startMs + MIN_CUE_MS / 2 && opts.atMs < cue.endMs - MIN_CUE_MS / 2) {
    mid = Math.round(opts.atMs);
  } else {
    const ratio = flat.length > 0 ? at / flat.length : 0.5;
    mid = Math.round(cue.startMs + duration * Math.min(0.9, Math.max(0.1, ratio)));
  }
  const a: Cue = { ...cue, endMs: mid, text: left };
  const b: Cue = { id: newId(), startMs: mid, endMs: cue.endMs, text: right };
  return [...cues.slice(0, i), a, b, ...cues.slice(i + 1)];
}

/** Merges a cue with the one after it (in start order). */
export function mergeWithNext(cues: Cue[], id: string): Cue[] {
  const sorted = sortCues(cues);
  const i = sorted.findIndex((c) => c.id === id);
  if (i < 0 || i === sorted.length - 1) return cues;
  const a = sorted[i];
  const b = sorted[i + 1];
  const text = [a.text.trim(), b.text.trim()].filter(Boolean).join(" ");
  const merged: Cue = { ...a, endMs: Math.max(a.endMs, b.endMs), text };
  return [...sorted.slice(0, i), merged, ...sorted.slice(i + 2)];
}

/** Shifts every cue by `deltaMs`. Cues pushed before 0 are clamped (and kept ≥ MIN_CUE_MS). */
export function offsetAll(cues: Cue[], deltaMs: number): Cue[] {
  return cues.map((c) => {
    let start = c.startMs + deltaMs;
    let end = c.endMs + deltaMs;
    if (start < 0) {
      start = 0;
      end = Math.max(end, MIN_CUE_MS);
    }
    return { ...c, startMs: Math.round(start), endMs: Math.round(end) };
  });
}

/** Ids of cues that overlap the next cue in start order. */
export function overlappingIds(cues: Cue[]): Set<string> {
  const sorted = sortCues(cues);
  const ids = new Set<string>();
  for (let i = 0; i < sorted.length - 1; i++) {
    if (sorted[i + 1].startMs < sorted[i].endMs) {
      ids.add(sorted[i].id);
      ids.add(sorted[i + 1].id);
    }
  }
  return ids;
}

/** Index of the cue showing at `ms` in a start-sorted list, or -1 (binary search). */
export function activeIndex(sorted: Cue[], ms: number): number {
  let lo = 0;
  let hi = sorted.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (sorted[mid].startMs <= ms) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  // Overlapping cues: walk back to the latest cue that still covers `ms`.
  for (let i = found; i >= 0 && i > found - 4; i--) {
    if (sorted[i].startMs <= ms && ms < sorted[i].endMs) return i;
  }
  return -1;
}

function nearestBoundary(text: string, from: number): number {
  if (text.length === 0) return 0;
  for (let d = 0; d < text.length; d++) {
    for (const p of [from - d, from + d]) {
      if (p > 0 && p < text.length && /\s/.test(text[p])) return p;
    }
  }
  return from;
}
