// Time formatting for the editor. The editor shows `HH:MM:SS.mmm` (dot, like most
// editors); SRT export uses a comma and is produced by the Rust side.

const pad = (n: number, w = 2) => String(Math.floor(n)).padStart(w, "0");

/** 00:01:02.345 */
export function formatTimecode(ms: number): string {
  const v = Math.max(0, Math.round(ms));
  const h = Math.floor(v / 3_600_000);
  const m = Math.floor(v / 60_000) % 60;
  const s = Math.floor(v / 1000) % 60;
  return `${pad(h)}:${pad(m)}:${pad(s)}.${pad(v % 1000, 3)}`;
}

/** Compact clock for transport: 1:02 or 1:02:03. */
export function formatClock(ms: number): string {
  const v = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(v / 3600);
  const m = Math.floor(v / 60) % 60;
  const s = v % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Human duration: "4 min 12 s", "1 h 3 min", "38 s". */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total / 60) % 60;
  const s = total % 60;
  if (h > 0) return `${h} h ${m} min`;
  if (m > 0) return s > 0 ? `${m} min ${s} s` : `${m} min`;
  return `${s} s`;
}

/** Seconds with one decimal for cue lengths: "2.4 s". */
export function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(1)} s`;
}

/**
 * Parses what people type into a time field. Accepts `HH:MM:SS.mmm`, `MM:SS.mmm`,
 * `SS.mmm`, comma or dot decimals. Returns null when the input isn't a time.
 */
export function parseTimecode(input: string): number | null {
  const t = input.trim().replace(",", ".");
  if (!/^\d+(:\d{1,2}){0,2}(\.\d{1,3})?$/.test(t)) return null;
  const [clock, frac = ""] = t.split(".");
  const parts = clock.split(":").map(Number);
  while (parts.length < 3) parts.unshift(0);
  const [h, m, s] = parts;
  if ((clock.includes(":") && (m > 59 || s > 59)) || [h, m, s].some(Number.isNaN)) return null;
  const ms = frac ? Number(frac.padEnd(3, "0")) : 0;
  return h * 3_600_000 + m * 60_000 + s * 1000 + ms;
}

/** Offset typed as seconds ("+1.5", "-0.25", "2") → milliseconds. */
export function parseOffsetSeconds(input: string): number | null {
  const t = input.trim().replace(",", ".");
  if (!/^[+-]?\d*(\.\d+)?$/.test(t) || t === "" || t === "+" || t === "-") return null;
  return Math.round(Number(t) * 1000);
}
