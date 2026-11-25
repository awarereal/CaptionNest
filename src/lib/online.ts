// Online speech recognition (Sarvam AI). Used instead of the offline models when the
// user has added a key and the spoken language is one Sarvam knows.
import type { Settings } from "./types";

/** Languages Sarvam's saaras:v4 recognizes (CaptionNest codes). */
export const ONLINE_LANGUAGES = new Set(["bn", "hi", "en", "ta", "te", "kn", "ml", "mr", "gu", "pa", "or", "as", "ur", "ne", "sa", "sd"]);

export const SARVAM_DASHBOARD = "https://dashboard.sarvam.ai";

export function hasOnlineKey(settings: Settings | null): boolean {
  return !!settings?.sarvamApiKey?.trim();
}

/** Whether a new job with this spoken language will be recognized online. */
export function onlineActive(settings: Settings | null, spoken: string): boolean {
  return hasOnlineKey(settings) && (settings?.onlineRecognition ?? true) && (spoken === "auto" || ONLINE_LANGUAGES.has(spoken));
}

/** "sk_ab…9lnQ": enough to recognise a key without showing it. */
export function maskKey(key: string): string {
  const k = key.trim();
  return k.length <= 10 ? "••••" : `${k.slice(0, 5)}…${k.slice(-4)}`;
}
