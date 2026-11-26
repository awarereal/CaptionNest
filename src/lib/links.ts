// Pure helpers for "Paste a video link" (unit-tested in links.test.ts).
import type { Storyboard } from "./types";

/** Something that looks like a video page address worth reading (for paste detection). */
export function looksLikeLink(text: string): boolean {
  const t = text.trim();
  if (!t || t.length > 2048 || /\s/.test(t)) return false;
  return (
    /^https?:\/\/[^/\s]+\.[^/\s]+/i.test(t) ||
    /^(www\.|m\.)?(youtube\.com|youtu\.be|facebook\.com|fb\.watch|tiktok\.com|instagram\.com|vimeo\.com|x\.com|twitter\.com)\//i.test(t)
  );
}

/** Which storyboard sheet holds the frame shown at `ms`, and its column and row there. */
export function frameAt(sb: Storyboard, ms: number): { url: string; col: number; row: number } {
  const perSheet = sb.columns * sb.rows;
  const last = sb.sheets.length * perSheet - 1;
  const index = Math.min(last, Math.max(0, Math.floor(ms / sb.frameMs)));
  const sheet = Math.floor(index / perSheet);
  const cell = index % perSheet;
  return { url: sb.sheets[sheet], col: cell % sb.columns, row: Math.floor(cell / sb.columns) };
}
