// Auto-sync captions: an offline model listens to the video, the captions are matched
// to what it heard, and their new times are shown for review before anything changes.
import { create } from "zustand";
import { toast } from "sonner";
import { api, toAppError } from "../lib/api";
import { sortCues } from "../lib/cueOps";
import { DEFAULT_MODEL_ID } from "../lib/models";
import type { AppError, Cue, FinishedEvent, ProgressEvent, Stage, SyncResult } from "../lib/types";
import { usePlayhead } from "./playhead";
import { useStore } from "./store";

export type SyncScope = "all" | "section";

interface SyncState {
  phase: "closed" | "options" | "running" | "preview" | "failed";
  scope: SyncScope;
  fromMs: number;
  toMs: number;
  /** The track being synced (kept even if the user switches tracks meanwhile). */
  trackId: string | null;
  stage: Stage | null;
  progress: number | null;
  jobId: string | null;
  result: SyncResult | null;
  error: AppError | null;
  open: () => void;
  close: () => void;
  setScope: (scope: SyncScope) => void;
  setRange: (fromMs: number, toMs: number) => void;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  back: () => void;
  apply: () => void;
  onProgress: (e: ProgressEvent) => void;
  onFinished: (e: FinishedEvent<unknown>) => void;
}

export const useSync = create<SyncState>((set, get) => ({
  phase: "closed",
  scope: "all",
  fromMs: 0,
  toMs: 0,
  trackId: null,
  stage: null,
  progress: null,
  jobId: null,
  result: null,
  error: null,

  open() {
    const st = useStore.getState();
    if (st.view !== "workspace" || !st.project || st.job || !st.activeTrackId) return;
    const track = st.history.present.find((t) => t.id === st.activeTrackId);
    const selected = track?.cues.find((c) => c.id === st.selectedCueId);
    // A section starts at the selected caption, or where the video is.
    const from = selected ? selected.startMs : Math.round(usePlayhead.getState().ms);
    set({
      phase: "options",
      scope: "all",
      fromMs: Math.max(0, from),
      toMs: st.project.video.durationMs,
      trackId: st.activeTrackId,
      result: null,
      error: null,
      jobId: null,
      stage: null,
      progress: null,
    });
  },

  close() {
    if (get().phase === "running") return;
    set({ phase: "closed", result: null, error: null });
  },

  setScope: (scope) => set({ scope }),
  setRange: (fromMs, toMs) => set({ fromMs, toMs }),

  async start() {
    const st = useStore.getState();
    const { trackId, scope, fromMs, toMs } = get();
    const track = st.history.present.find((t) => t.id === trackId);
    const project = st.project;
    if (!track || !project || !st.media) return;
    set({ phase: "running", stage: "preparing", progress: null, error: null, result: null, jobId: null });
    try {
      const jobId = await api.startSync({
        videoPath: st.media.path,
        audioStreamIndex: project.video.audioStreamIndex ?? st.audioStream ?? 0,
        durationMs: project.video.durationMs,
        language: track.language,
        cues: track.cues.map((c) => ({ id: c.id, startMs: c.startMs, endMs: c.endMs, text: c.text })),
        fromMs: scope === "section" ? Math.min(fromMs, toMs) : null,
        toMs: scope === "section" ? Math.max(fromMs, toMs) : null,
        transcribeModelId: st.settings?.transcribeModelId ?? DEFAULT_MODEL_ID,
        useGpu: st.settings?.useGpu ?? true,
      });
      if (get().phase !== "running") {
        await api.cancelJob(jobId);
        return;
      }
      set({ jobId });
    } catch (e) {
      set({ phase: "failed", error: toAppError(e) });
    }
  },

  async cancel() {
    const { jobId } = get();
    set({ phase: "options", jobId: null, stage: null, progress: null });
    if (jobId) await api.cancelJob(jobId);
  },

  back: () => set({ phase: "options", result: null, error: null }),

  apply() {
    const { result, trackId } = get();
    const st = useStore.getState();
    const track = st.history.present.find((t) => t.id === trackId);
    if (!result || !track) return;
    const byId = new Map(result.cues.map((c) => [c.id, c]));
    let moved = 0;
    let review = 0;
    const cues: Cue[] = track.cues.map((c) => {
      const s = byId.get(c.id);
      if (!s || s.status === "outside") return c;
      const flag = s.status === "estimated" || s.status === "unmatched";
      if (flag) review += 1;
      if (s.startMs !== c.startMs || s.endMs !== c.endMs) moved += 1;
      // Only the times change (and the review mark): text and order stay as they are.
      return { ...c, startMs: s.startMs, endMs: s.endMs, timingCheck: flag ? true : undefined };
    });
    if (st.activeTrackId !== track.id) st.setActiveTrack(track.id);
    st.replaceCues(sortCues(cues));
    set({ phase: "closed", result: null });
    const what = moved === 1 ? "1 caption moved" : `${moved} captions moved`;
    toast(`${what} to match the speech${review > 0 ? ` · ${review} to review` : ""}`, {
      duration: 9000,
      action: { label: "Undo", onClick: () => useStore.getState().undo() },
    });
  },

  onProgress(e) {
    const s = get();
    if (s.phase !== "running" || (s.jobId && s.jobId !== e.jobId)) return;
    set({ jobId: e.jobId, stage: e.stage, progress: e.progress });
  },

  onFinished(e) {
    const s = get();
    if (s.phase !== "running" || (s.jobId && s.jobId !== e.jobId)) return;
    if (e.outcome.status === "success") {
      set({ phase: "preview", result: e.outcome.result as SyncResult, jobId: null });
    } else if (e.outcome.status === "failed") {
      set({ phase: "failed", error: e.outcome.error, jobId: null });
    } else {
      set({ phase: "options", jobId: null });
    }
  },
}));
