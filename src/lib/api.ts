// Typed wrappers around the Rust commands and events.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppError,
  AppInfo,
  Cue,
  DownloadEvent,
  FinishedEvent,
  GenerateResult,
  LoadedProject,
  MediaInfo,
  ModelState,
  PlanInfo,
  Preview,
  FontImportResult,
  Heard,
  ProgressEvent,
  Project,
  Settings,
  Track,
  UserFont,
  ValidationReport,
  VoiceStatus,
} from "./types";

/** Normalizes anything thrown by `invoke` into an AppError. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as AppError;
  return { code: "internal", message: "Something unexpected went wrong.", detail: String(e) };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

export interface GenerateRequest {
  videoPath: string;
  audioStreamIndex: number;
  durationMs: number;
  spokenLanguage: string;
  captionLanguage: string;
  transcribeModelId: string;
  translateModelId: string | null;
  useGpu: boolean;
  maxLineChars: number;
}

export interface TranslateRequest {
  videoPath: string;
  audioStreamIndex: number;
  durationMs: number;
  sourceTrack: Track;
  targetLanguage: string;
  translateModelId: string | null;
  useGpu: boolean;
  maxLineChars: number;
}

export interface BurnRequest {
  videoPath: string;
  audioStreamIndex: number | null;
  outputPath: string;
  framesId: string;
  blank: string;
  cues: { startMs: number; endMs: number; file: string }[];
}

export interface SyncRequest {
  videoPath: string;
  audioStreamIndex: number;
  durationMs: number;
  language: string;
  cues: Cue[];
  fromMs: number | null;
  toMs: number | null;
  transcribeModelId: string;
  useGpu: boolean;
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  showWindow: () => call<void>("show_main_window"),
  getSettings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  probeMedia: (path: string) => call<MediaInfo>("probe_media", { path }),
  startPreview: (path: string, audioStreamIndex: number | null) =>
    call<string>("start_preview", { path, audioStreamIndex }),

  startGenerate: (request: GenerateRequest) => call<string>("start_generate", { request }),
  startTranslate: (request: TranslateRequest) => call<string>("start_translate", { request }),
  cancelJob: (jobId: string) => call<void>("cancel_job", { jobId }),
  planTranslation: (source: string, target: string) => call<PlanInfo>("plan_translation", { source, target }),

  modelState: () => call<ModelState>("model_state"),
  downloadModel: (id: string) => call<void>("download_model", { id }),
  cancelDownload: (id: string) => call<void>("cancel_download", { id }),
  removeModel: (id: string) => call<void>("remove_model", { id }),

  validateCues: (cues: Cue[]) => call<ValidationReport>("validate_cues", { cues }),
  fixOverlaps: (cues: Cue[], minMs = 300) => call<Cue[]>("fix_overlaps", { cues, minMs }),
  exportSrt: (path: string, cues: Cue[]) =>
    call<{ path: string; cueCount: number; bytes: number }>("export_srt", { path, cues }),
  importSrt: (path: string) => call<Cue[]>("import_srt", { path }),

  checkOnlineKey: (key: string) => call<void>("check_online_key", { key }),
  /** Checks the Sarvam key, saves it in Rust and returns the settings (with a masked hint). */
  saveOnlineKey: (key: string) => call<Settings>("save_online_key", { key }),
  removeOnlineKey: () => call<Settings>("remove_online_key"),
  startSync: (request: SyncRequest) => call<string>("start_sync", { request }),

  burnBegin: () => call<string>("burn_begin"),
  /** Sends one caption image as raw bytes (no JSON encoding of large arrays). */
  burnFrame: async (framesId: string, file: string, png: Uint8Array) => {
    try {
      await invoke("burn_frame", png, { headers: { "x-burn-id": framesId, "x-file": file } });
    } catch (e) {
      throw toAppError(e);
    }
  },
  startBurn: (request: BurnRequest) => call<string>("start_burn", { request }),

  saveProject: (path: string, project: Project) => call<void>("save_project", { path, project }),
  loadProject: (path: string) => call<LoadedProject>("load_project", { path }),
  forgetRecent: (path: string) => call<void>("forget_recent", { path }),
  pathExists: (path: string) => call<boolean>("path_exists", { path }),

  listFonts: () => call<UserFont[]>("list_fonts"),
  importFonts: (paths: string[]) => call<FontImportResult>("import_fonts", { paths }),
  /** The bytes of one added font file. */
  fontFile: (file: string) => call<ArrayBuffer>("font_file", { file }),
  removeFont: (id: string) => call<UserFont[]>("remove_font", { id }),

  voiceStatus: () => call<VoiceStatus>("voice_status"),
  saveVoiceKey: (key: string) => call<VoiceStatus>("save_voice_key", { key }),
  removeVoiceKey: () => call<VoiceStatus>("remove_voice_key"),
  /** Sends one microphone recording as raw bytes and returns what was said. */
  transcribeVoice: async (audio: Uint8Array, mime: string, language: string): Promise<Heard> => {
    try {
      return await invoke<Heard>("transcribe_voice", audio, { headers: { "x-mime": mime, "x-language": language } });
    } catch (e) {
      throw toAppError(e);
    }
  },
};

export const events = {
  onJobProgress: (fn: (e: ProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ProgressEvent>("job-progress", (ev) => fn(ev.payload)),
  onJobFinished: <T = GenerateResult | Preview>(fn: (e: FinishedEvent<T>) => void): Promise<UnlistenFn> =>
    listen<FinishedEvent<T>>("job-finished", (ev) => fn(ev.payload)),
  onDownload: (fn: (e: DownloadEvent) => void): Promise<UnlistenFn> =>
    listen<DownloadEvent>("download-progress", (ev) => fn(ev.payload)),
};

/** URL the web view can stream a local file from (range requests supported). */
export const mediaUrl = (path: string) => convertFileSrc(path);
