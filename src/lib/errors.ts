// Turns backend errors into a title, a plain explanation and what the user can do next.
import type { AppError } from "./types";

export type RecoveryAction =
  | "retry"
  | "choose-video"
  | "choose-track"
  | "open-models"
  | "open-languages"
  | "preview"
  | "set-language"
  | "free-space"
  | "open-online"
  | "open-voice"
  | "show-details";

export interface Described {
  title: string;
  message: string;
  actions: RecoveryAction[];
}

export function describeError(e: AppError): Described {
  const m = e.message;
  switch (e.code) {
    case "file_not_found":
      return { title: "File not found", message: m, actions: ["choose-video"] };
    case "no_audio":
      return { title: "No sound in this video", message: m, actions: ["choose-video", "choose-track"] };
    case "corrupt_media":
      return { title: "This file can't be read", message: m, actions: ["choose-video"] };
    case "unsupported_media":
      return { title: "Unsupported video format", message: m, actions: ["choose-video"] };
    case "no_speech":
      return {
        title: "No speech found",
        message: `${m} If someone does speak, choose the spoken language instead of automatic detection, or pick another sound track.`,
        actions: ["set-language", "choose-track", "retry"],
      };
    case "disk_full":
      return { title: "Not enough disk space", message: m, actions: ["free-space", "retry"] };
    case "model_missing":
      return {
        title: e.detail?.startsWith("argos:") ? "Language pack needed" : "Model needed",
        message: m,
        actions: [e.detail?.startsWith("argos:") ? "open-languages" : "open-models"],
      };
    case "download_failed":
    case "network":
      return { title: "Download problem", message: m, actions: ["retry"] };
    case "checksum_mismatch":
      return { title: "Download was damaged", message: m, actions: ["retry"] };
    case "tool_missing":
      return { title: "CaptionNest needs repair", message: m, actions: ["show-details"] };
    case "tool_failed":
      return { title: "Processing stopped", message: `${m} Try again; if it keeps happening, turn off GPU acceleration in Settings.`, actions: ["retry", "show-details"] };
    case "translation_unavailable":
      return { title: "Translation not available", message: m, actions: ["open-languages"] };
    case "translation_failed":
      return { title: "Translation stopped", message: m, actions: ["retry", "show-details"] };
    case "project_invalid":
      return { title: "Can't open this project", message: m, actions: [] };
    case "project_too_new":
      return { title: "Project needs a newer version", message: m, actions: [] };
    case "export_invalid":
      return { title: "Captions need fixing", message: m, actions: [] };
    case "io":
      return { title: "File problem", message: m, actions: ["retry"] };
    case "cancelled":
      return { title: "Stopped", message: m, actions: [] };
    case "online_key":
      return { title: "Sarvam key not accepted", message: m, actions: ["open-online"] };
    case "online_credits":
      return { title: "Sarvam credits used up", message: m, actions: ["open-online"] };
    case "voice_key":
      return { title: "Deepgram key needed", message: m, actions: ["open-voice"] };
    case "voice_credits":
      return { title: "Deepgram credits used up", message: m, actions: ["open-voice"] };
    case "voice_failed":
      return { title: "Voice typing stopped", message: m, actions: ["retry", "show-details"] };
    case "font_invalid":
      return { title: "Font can't be used", message: m, actions: [] };
    case "online_failed":
      return { title: "Online recognition stopped", message: m, actions: ["retry", "open-online", "show-details"] };
    default:
      return { title: "Something went wrong", message: m, actions: ["retry", "show-details"] };
  }
}
