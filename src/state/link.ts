// "Paste a video link": read the link, choose the part and quality, fetch it, then open
// it for captions. Reading and fetching run in Rust (src-tauri/src/links/).
import { create } from "zustand";
import { toast } from "sonner";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, toAppError } from "../lib/api";
import { openPath, pickOpen } from "../lib/fileFlows";
import type { AppError, FinishedEvent, LinkInfo, LinkQuality, LinkResult, LinkToolsState, ProgressEvent, QualityChoice } from "../lib/types";
import { useStore } from "./store";

export const LINK_TOOLS_ID = "link-tools";

export { looksLikeLink } from "../lib/links";

type Phase = "closed" | "setup" | "idle" | "reading" | "ready" | "fetching" | "failed";

interface LinkState {
  phase: Phase;
  /** The dialog was closed during a download: it carries on, shown as a small notice. */
  hidden: boolean;
  /** What's in the link field. */
  url: string;
  tools: LinkToolsState | null;
  info: LinkInfo | null;
  whole: boolean;
  fromMs: number;
  toMs: number;
  quality: LinkQuality;
  jobId: string | null;
  stage: "downloading" | "finishing";
  progress: number | null;
  received: number;
  estimate: number;
  speed: number;
  startedAt: number;
  error: AppError | null;
  /** Which step failed: reading the link, setting up, or fetching the video. */
  failedAt: "read" | "setup" | "fetch" | null;

  open: (url?: string) => Promise<void>;
  close: () => void;
  setUrl: (url: string) => void;
  read: (url?: string) => Promise<void>;
  setup: () => Promise<void>;
  afterSetup: () => Promise<void>;
  setWhole: (whole: boolean) => void;
  setPart: (fromMs: number, toMs: number) => void;
  setQuality: (q: LinkQuality) => void;
  chooseFolder: () => Promise<void>;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  retry: () => void;
  onProgress: (e: ProgressEvent) => void;
  onFinished: (e: FinishedEvent<unknown>) => void;
}

/** A first part to offer: from where the link starts (or 0), one minute long. */
const DEFAULT_PART_MS = 60_000;

export function folderOf(s: LinkState): string {
  return useStore.getState().settings?.clipFolder || s.tools?.defaultFolder || "";
}

export function choiceOf(s: Pick<LinkState, "info" | "quality">): QualityChoice | undefined {
  return s.info?.choices.find((c) => c.quality === s.quality);
}

/** About how big the result will be, for the chosen part and quality. */
export function estimateOf(s: Pick<LinkState, "info" | "quality" | "whole" | "fromMs" | "toMs">): number | null {
  const c = choiceOf(s);
  if (!c) return null;
  if (s.whole) return c.wholeBytes;
  return c.partBytesPerSec ? Math.round((c.partBytesPerSec * (s.toMs - s.fromMs)) / 1000) : null;
}

const NOTICE = "link-fetch";

/** The small notice shown while a download runs with its dialog closed. */
function notice(s: LinkState) {
  const pct = s.progress != null ? ` ${Math.round(s.progress * 100)}%` : "";
  toast.loading(s.stage === "finishing" ? "Finishing the video…" : `Getting the video…${pct}`, {
    id: NOTICE,
    description: s.info?.title,
    action: { label: "Show", onClick: () => useLink.setState({ hidden: false }) },
  });
}

export const useLink = create<LinkState>((set, get) => ({
  phase: "closed",
  hidden: false,
  url: "",
  tools: null,
  info: null,
  whole: true,
  fromMs: 0,
  toMs: 0,
  quality: "editing",
  jobId: null,
  stage: "downloading",
  progress: null,
  received: 0,
  estimate: 0,
  speed: 0,
  startedAt: 0,
  error: null,
  failedAt: null,

  async open(url) {
    if (get().phase === "fetching") {
      // One video at a time: show the one that's on its way.
      toast.dismiss(NOTICE);
      set({ hidden: false });
      return;
    }
    const tools = await api.linkToolsState().catch(() => null);
    const text = (url ?? "").trim();
    set({ tools, url: text, info: null, error: null, failedAt: null, jobId: null, progress: null });
    if (!tools?.installed) {
      set({ phase: "setup" });
      return;
    }
    if (text) await get().read(text);
    else set({ phase: "idle" });
  },

  close() {
    const { phase } = get();
    if (phase === "fetching") {
      // Keeps going in the background; the video still opens when it's ready.
      set({ hidden: true });
      notice(get());
      return;
    }
    if (phase === "reading") void api.cancelLinkProbe();
    if (phase === "setup" && useStore.getState().downloads[LINK_TOOLS_ID]?.state === "downloading") void api.cancelDownload(LINK_TOOLS_ID);
    set({ phase: "closed" });
  },

  setUrl: (url) => set({ url }),

  async read(url) {
    const text = (url ?? get().url).trim();
    if (!text) return;
    set({ phase: "reading", url: text, info: null, error: null, failedAt: null });
    try {
      const info = await api.probeLink(text);
      if (get().phase !== "reading" || get().url !== text) return; // closed or replaced meanwhile
      const duration = info.durationMs ?? 0;
      const start = info.startMs != null && info.startMs < duration ? info.startMs : 0;
      // A link that starts mid-video (YouTube's t=) most likely wants a part from there.
      const whole = info.startMs == null || !duration;
      const quality = (["editing", "best", "audio"] as LinkQuality[]).find((q) => info.choices.find((c) => c.quality === q)?.available) ?? "editing";
      set({
        phase: "ready",
        info,
        whole,
        fromMs: start,
        toMs: Math.min(duration, start + DEFAULT_PART_MS) || duration,
        quality,
      });
    } catch (e) {
      const err = toAppError(e);
      if (err.code === "cancelled" || get().phase !== "reading") return;
      if (err.code === "tool_missing") {
        set({ phase: "setup" });
        return;
      }
      set({ phase: "failed", error: err, failedAt: "read" });
    }
  },

  async setup() {
    set({ error: null, failedAt: null });
    try {
      await api.downloadModel(LINK_TOOLS_ID);
    } catch (e) {
      set({ phase: "failed", error: toAppError(e), failedAt: "setup" });
    }
  },

  async afterSetup() {
    const tools = await api.linkToolsState().catch(() => null);
    set({ tools });
    if (!tools?.installed) return;
    if (get().url) await get().read();
    else set({ phase: "idle" });
  },

  setWhole: (whole) => set({ whole }),
  setPart: (fromMs, toMs) => set({ fromMs: Math.max(0, Math.round(fromMs)), toMs: Math.round(toMs) }),
  setQuality: (quality) => set({ quality }),

  async chooseFolder() {
    const current = folderOf(get());
    const picked = await pickOpen({ directory: true, multiple: false, defaultPath: current || undefined, title: "Save videos from links in…" });
    if (picked) await useStore.getState().updateSettings({ clipFolder: picked });
  },

  async start() {
    const s = get();
    if (!s.info || s.phase !== "ready") return;
    const estimate = estimateOf(s);
    set({ phase: "fetching", progress: estimate ? 0 : null, received: 0, estimate: estimate ?? 0, speed: 0, stage: "downloading", startedAt: Date.now(), error: null, failedAt: null });
    try {
      const jobId = await api.startLink({
        key: s.info.key,
        url: s.info.url,
        quality: s.quality,
        fromMs: s.whole ? null : s.fromMs,
        toMs: s.whole ? null : s.toMs,
        folder: folderOf(s),
        estimateBytes: estimate,
      });
      set({ jobId });
    } catch (e) {
      set({ phase: "failed", error: toAppError(e), failedAt: "fetch" });
    }
  },

  async cancel() {
    const { jobId } = get();
    if (jobId) await api.cancelJob(jobId);
    else set({ phase: "ready" });
  },

  retry() {
    const { failedAt } = get();
    if (failedAt === "fetch" && get().info) {
      set({ phase: "ready", error: null, failedAt: null });
      void get().start();
    } else if (failedAt === "setup") {
      set({ phase: "setup", error: null, failedAt: null });
      void get().setup();
    } else {
      void get().read();
    }
  },

  onProgress(e) {
    if (e.jobId !== get().jobId) return;
    if (e.stage === "finishing") {
      set({ stage: "finishing", progress: null });
      if (get().hidden) notice(get());
      return;
    }
    const [received, estimate, speed] = (e.detail ?? "").split("|").map(Number);
    set({
      stage: "downloading",
      progress: e.progress,
      received: Number.isFinite(received) ? received : get().received,
      estimate: estimate || get().estimate,
      speed: Number.isFinite(speed) ? speed : get().speed,
    });
    if (get().hidden) notice(get());
  },

  onFinished(e) {
    if (e.jobId !== get().jobId) return;
    toast.dismiss(NOTICE);
    set({ hidden: false });
    const o = e.outcome;
    if (o.status === "success") {
      const r = o.result as LinkResult;
      set({ phase: "closed", jobId: null });
      const folder = r.path.replace(/[\\/][^\\/]*$/, "");
      toast.success("Video saved", {
        description: folder,
        action: { label: "Show", onClick: () => void revealItemInDir(r.path) },
      });
      // Straight on to captions (asks first if there are unsaved caption edits).
      void openPath(r.path);
    } else if (o.status === "failed") {
      set({ phase: "failed", error: o.error, failedAt: "fetch", jobId: null });
    } else {
      set({ phase: "ready", jobId: null, progress: null });
    }
  },
}));
