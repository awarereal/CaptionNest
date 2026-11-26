// Online speech recognition. Sarvam AI writes the words (fewest mistakes in Bangla, and
// it doesn't skip speech under music); Deepgram times every word, and writes captions on
// its own where Sarvam can't. The choice mirrors `pipeline::generate` in Rust.
import type { Settings } from "./types";

/** Languages Sarvam's saaras:v4 recognizes (CaptionNest codes). */
export const ONLINE_LANGUAGES = new Set(["bn", "hi", "en", "ta", "te", "kn", "ml", "mr", "gu", "pa", "or", "as", "ur", "ne", "sa", "sd"]);

/** Languages Deepgram's Nova-3 writes captions for (Rust: voice::caption_language). */
export const DEEPGRAM_LANGUAGES = new Set(["bn", "en", "hi", "es", "fr", "de", "pt", "it", "nl", "ja", "ru"]);

export const SARVAM_DASHBOARD = "https://dashboard.sarvam.ai";

/** Settings > Online recognition, when both keys are saved. */
export type OnlineEngine = "both" | "sarvam" | "deepgram";

/** What makes a new job's captions: Sarvam's words with Deepgram's timing, one service, or offline (null). */
export type OnlinePlan = "sarvam+deepgram" | "sarvam" | "deepgram" | null;

export function engineSetting(settings: Settings | null): OnlineEngine {
  const e = settings?.onlineEngine;
  return e === "sarvam" || e === "deepgram" ? e : "both";
}

/** A key for any online service is saved. */
export function hasOnlineKey(settings: Settings | null, deepgramKey = false): boolean {
  return !!settings?.sarvamKeyHint || deepgramKey;
}

/**
 * The online plan for a new job with this spoken language. `deepgramKey`: a Deepgram key
 * is saved (Settings > Voice typing). `canDetect`: an offline model is installed to detect
 * the language when it's "auto" (Deepgram must be told the language).
 */
export function onlinePlan(settings: Settings | null, deepgramKey: boolean, spoken: string, canDetect: boolean): OnlinePlan {
  if (!(settings?.onlineRecognition ?? true)) return null;
  const engine = engineSetting(settings);
  const sarvam = !!settings?.sarvamKeyHint && (spoken === "auto" || ONLINE_LANGUAGES.has(spoken));
  const deepgram = deepgramKey && engine !== "sarvam" && (spoken === "auto" ? canDetect : DEEPGRAM_LANGUAGES.has(spoken));
  if (sarvam && engine !== "deepgram") return deepgram ? "sarvam+deepgram" : "sarvam";
  if (deepgram) return "deepgram";
  return sarvam ? "sarvam" : null;
}

/** Whether Auto-sync listens with Deepgram (Rust: start_sync) for captions in this language. */
export function syncWithDeepgram(settings: Settings | null, deepgramKey: boolean, language: string | undefined): boolean {
  return deepgramKey && (settings?.onlineRecognition ?? true) && engineSetting(settings) !== "sarvam" && DEEPGRAM_LANGUAGES.has(language ?? "");
}

/** Who the plan sends the video's sound to, in words. */
export function planServices(plan: OnlinePlan): string {
  return plan === "sarvam+deepgram" ? "Sarvam and Deepgram" : plan === "deepgram" ? "Deepgram" : plan === "sarvam" ? "Sarvam" : "";
}
