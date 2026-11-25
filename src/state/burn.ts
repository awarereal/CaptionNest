// Exporting a video with the captions drawn in. Two phases: the app draws every caption
// as an image (same renderer as the preview), then FFmpeg lays them over the video.
import { create } from "zustand";
import { api, toAppError } from "../lib/api";
import { ensureFonts, type CaptionStyle } from "../lib/captionStyle";
import { captionPng } from "../lib/renderCaption";
import { sortCues } from "../lib/cueOps";
import type { AppError, Cue, FinishedEvent, ProgressEvent } from "../lib/types";

export interface BurnArgs {
  videoPath: string;
  audioStreamIndex: number | null;
  outputPath: string;
  cues: Cue[];
  style: CaptionStyle;
  width: number;
  height: number;
}

interface BurnState {
  phase: "idle" | "drawing" | "rendering" | "done" | "failed";
  /** 0-1 within the current phase. */
  progress: number;
  jobId: string | null;
  captions: number;
  result: { path: string; encoder: string } | null;
  error: AppError | null;
  run: (args: BurnArgs) => Promise<void>;
  cancel: () => Promise<void>;
  reset: () => void;
  onProgress: (e: ProgressEvent) => void;
  onFinished: (e: FinishedEvent<unknown>) => void;
}

/** Bumped by every new run or cancel, so a stopped drawing loop knows to quit. */
let runToken = 0;

export const useBurn = create<BurnState>((set, get) => ({
  phase: "idle",
  progress: 0,
  jobId: null,
  captions: 0,
  result: null,
  error: null,

  async run(args) {
    const token = ++runToken;
    const cues = sortCues(args.cues).filter((c) => c.text.trim());
    set({ phase: "drawing", progress: 0, jobId: null, captions: cues.length, result: null, error: null });
    try {
      const framesId = await api.burnBegin();
      await ensureFonts(args.style, cues.map((c) => c.text).join(" "));
      await api.burnFrame(framesId, "blank.png", await captionPng("", args.style, args.width, args.height));
      const frames = [];
      for (let i = 0; i < cues.length; i++) {
        if (token !== runToken) return;
        const file = `c${String(i).padStart(5, "0")}.png`;
        await api.burnFrame(framesId, file, await captionPng(cues[i].text, args.style, args.width, args.height));
        frames.push({ startMs: cues[i].startMs, endMs: cues[i].endMs, file });
        set({ progress: (i + 1) / Math.max(1, cues.length) });
      }
      if (token !== runToken) return;
      const jobId = await api.startBurn({
        videoPath: args.videoPath,
        audioStreamIndex: args.audioStreamIndex,
        outputPath: args.outputPath,
        framesId,
        blank: "blank.png",
        cues: frames,
      });
      if (token !== runToken) {
        await api.cancelJob(jobId);
        return;
      }
      set({ phase: "rendering", progress: 0, jobId });
    } catch (e) {
      if (token === runToken) set({ phase: "failed", error: toAppError(e) });
    }
  },

  async cancel() {
    const { phase, jobId } = get();
    runToken++;
    if (phase === "rendering" && jobId) await api.cancelJob(jobId);
    set({ phase: "idle", progress: 0, jobId: null });
  },

  reset: () => set({ phase: "idle", progress: 0, jobId: null, result: null, error: null }),

  onProgress(e) {
    if (e.jobId === get().jobId && get().phase === "rendering") set({ progress: e.progress ?? get().progress });
  },

  onFinished(e) {
    if (e.jobId !== get().jobId) return;
    if (e.outcome.status === "success") {
      set({ phase: "done", progress: 1, result: e.outcome.result as { path: string; encoder: string } });
    } else if (e.outcome.status === "failed") {
      set({ phase: "failed", error: e.outcome.error });
    } else {
      set({ phase: "idle", progress: 0, jobId: null });
    }
  },
}));
