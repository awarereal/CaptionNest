// Application state: what's on screen, the open project, edits with undo/redo,
// background jobs and model downloads.
import { create } from "zustand";
import { api, mediaUrl, toAppError } from "../lib/api";
import * as ops from "../lib/cueOps";
import { createHistory, record, redo as redoH, undo as undoH, type History } from "../lib/history";
import { DEFAULT_MODEL_ID, MODEL_INFO } from "../lib/models";
import type {
  AppError,
  AppInfo,
  Cue,
  DownloadEvent,
  FinishedEvent,
  GenerateResult,
  MediaInfo,
  ModelState,
  Preview,
  ProgressEvent,
  Project,
  Settings,
  Stage,
  Track,
} from "../lib/types";

export type View = "loading" | "setup" | "home" | "prepare" | "workspace";

export interface JobView {
  id: string | null;
  kind: "generate" | "translate";
  stage: Stage;
  progress: number | null;
  detail?: string;
  startedAt: number;
  /** When the current stage began (for time-left estimates). */
  stageStartedAt: number;
  stages: Stage[];
  targetLanguage?: string;
}

export interface PreviewState {
  status: "original" | "converting" | "ready" | "audio-only" | "failed";
  url: string | null;
  jobId?: string;
  progress?: number;
  error?: AppError;
}

export type Panel = null | "settings" | "help" | "about";

interface Store {
  view: View;
  info: AppInfo | null;
  settings: Settings | null;
  models: ModelState | null;
  downloads: Record<string, DownloadEvent>;

  media: MediaInfo | null;
  audioStream: number | null;
  preview: PreviewState;
  opening: boolean;

  project: Project | null;
  projectPath: string | null;
  dirty: boolean;
  videoMissing: boolean;
  history: History<Track[]>;
  activeTrackId: string | null;
  selectedCueId: string | null;

  job: JobView | null;
  jobError: AppError | null;
  warnings: string[];
  panel: Panel;
  settingsSection: string | null;

  init: () => Promise<void>;
  setView: (v: View) => void;
  openPanel: (p: Panel, section?: string | null) => void;
  updateSettings: (patch: Partial<Settings>) => Promise<void>;
  refreshModels: () => Promise<void>;
  download: (id: string) => Promise<void>;
  cancelDownload: (id: string) => Promise<void>;
  removeModel: (id: string) => Promise<void>;
  onDownload: (e: DownloadEvent) => void;

  openVideo: (path: string) => Promise<void>;
  setAudioStream: (streamIndex: number) => void;
  requestPreview: () => Promise<void>;
  startGenerate: () => Promise<void>;
  startTranslate: (target: string) => Promise<void>;
  cancelJob: () => Promise<void>;
  dismissJobError: () => void;
  onJobProgress: (e: ProgressEvent) => void;
  onJobFinished: (e: FinishedEvent<GenerateResult | Preview>) => void;

  loadProject: (path: string) => Promise<void>;
  saveProject: (path: string) => Promise<void>;
  relinkVideo: (path: string) => Promise<{ ok: boolean; durationMismatch?: boolean }>;
  closeProject: () => void;
  backToOptions: () => void;

  setActiveTrack: (id: string) => void;
  selectCue: (id: string | null) => void;
  setCueText: (id: string, text: string) => void;
  markChecked: (id: string) => void;
  retimeCue: (id: string, startMs: number, endMs: number) => void;
  addCue: (atMs: number) => string | null;
  deleteCue: (id: string) => void;
  splitCue: (id: string, opts: { caret?: number; atMs?: number }) => void;
  mergeCue: (id: string) => void;
  offsetAll: (deltaMs: number) => void;
  replaceCues: (cues: Cue[]) => void;
  deleteTrack: (id: string) => void;
  undo: () => void;
  redo: () => void;
}

export const useStore = create<Store>((set, get) => {
  /** Applies a change to the active track's cues as one undoable step. */
  const commit = (fn: (cues: Cue[]) => Cue[], key: string | null = null) => {
    const { history, activeTrackId } = get();
    const tracks = history.present;
    const idx = tracks.findIndex((t) => t.id === activeTrackId);
    if (idx < 0) return;
    const nextCues = fn(tracks[idx].cues);
    if (nextCues === tracks[idx].cues) return;
    const nextTracks = tracks.map((t, i) => (i === idx ? { ...t, cues: nextCues } : t));
    set({ history: record(history, nextTracks, key), dirty: true });
  };

  return {
    view: "loading",
    info: null,
    settings: null,
    models: null,
    downloads: {},
    media: null,
    audioStream: null,
    preview: { status: "original", url: null },
    opening: false,
    project: null,
    projectPath: null,
    dirty: false,
    videoMissing: false,
    history: createHistory<Track[]>([]),
    activeTrackId: null,
    selectedCueId: null,
    job: null,
    jobError: null,
    warnings: [],
    panel: null,
    settingsSection: null,

    async init() {
      const [info, settings, models] = await Promise.all([api.appInfo(), api.getSettings(), api.modelState()]);
      const anyInstalled = models.whisper.some((m) => m.installed && !m.language);
      let s = settings;
      if (!s.transcribeModelId || !models.whisper.find((m) => m.id === s.transcribeModelId && !m.language)?.installed) {
        const pick = models.whisper.find((m) => m.installed && m.id === DEFAULT_MODEL_ID) ?? models.whisper.find((m) => m.installed && !m.language);
        if (pick) s = { ...s, transcribeModelId: pick.id };
      }
      set({ info, settings: s, models, view: anyInstalled || s.setupComplete ? "home" : "setup" });
      if (s !== settings) void api.saveSettings(s);
    },

    setView: (view) => set({ view }),
    openPanel: (panel, section = null) => set({ panel, settingsSection: section }),

    async updateSettings(patch) {
      const cur = get().settings;
      if (!cur) return;
      const next = { ...cur, ...patch };
      set({ settings: next });
      await api.saveSettings(next);
    },

    async refreshModels() {
      const models = await api.modelState();
      set({ models });
      const s = get().settings;
      if (s && (!s.transcribeModelId || !models.whisper.find((m) => m.id === s.transcribeModelId)?.installed)) {
        const pick = models.whisper.find((m) => m.installed && m.id === DEFAULT_MODEL_ID) ?? models.whisper.find((m) => m.installed && !m.language);
        if (pick || s.transcribeModelId) await get().updateSettings({ transcribeModelId: pick?.id ?? null });
      }
    },

    async download(id) {
      set((st) => ({ downloads: { ...st.downloads, [id]: { id, state: "downloading", received: 0, total: 0, bytesPerSec: 0 } } }));
      try {
        await api.downloadModel(id);
      } catch (e) {
        set((st) => ({ downloads: { ...st.downloads, [id]: { id, state: "failed", received: 0, total: 0, bytesPerSec: 0, error: toAppError(e) } } }));
      }
    },

    async cancelDownload(id) {
      await api.cancelDownload(id);
    },

    async removeModel(id) {
      await api.removeModel(id);
      await get().refreshModels();
    },

    onDownload(e) {
      set((st) => ({ downloads: { ...st.downloads, [e.id]: e } }));
      if (e.state === "done" || e.state === "cancelled" || e.state === "failed") {
        void get().refreshModels();
        if (e.state !== "failed") {
          setTimeout(() => {
            set((st) => {
              const d = { ...st.downloads };
              if (d[e.id]?.state === e.state) delete d[e.id];
              return { downloads: d };
            });
          }, 1500);
        }
      }
    },

    async openVideo(path) {
      set({ opening: true });
      try {
        const media = await api.probeMedia(path);
        const track = media.defaultAudio !== null ? media.audioTracks[media.defaultAudio] : null;
        set({
          media,
          audioStream: track?.streamIndex ?? null,
          preview: { status: "original", url: mediaUrl(media.path) },
          project: null,
          projectPath: null,
          dirty: false,
          videoMissing: false,
          history: createHistory<Track[]>([]),
          activeTrackId: null,
          selectedCueId: null,
          jobError: null,
          warnings: [],
          view: "prepare",
        });
      } finally {
        set({ opening: false });
      }
    },

    setAudioStream: (streamIndex) => set({ audioStream: streamIndex }),

    async requestPreview() {
      const { media, audioStream } = get();
      if (!media) return;
      set({ preview: { status: "converting", url: null, progress: 0 } });
      try {
        const jobId = await api.startPreview(media.path, audioStream);
        set((st) => ({ preview: { ...st.preview, jobId } }));
      } catch (e) {
        set({ preview: { status: "failed", url: null, error: toAppError(e) } });
      }
    },

    async startGenerate() {
      const { media, audioStream, settings } = get();
      if (!media || audioStream === null || !settings) return;
      const caption = settings.captionLanguage;
      const stages: Stage[] = ["extracting", "transcribing"];
      if (caption !== "same" && caption !== settings.spokenLanguage) stages.push("translating");
      set({
        job: { id: null, kind: "generate", stage: "preparing", progress: null, startedAt: Date.now(), stageStartedAt: Date.now(), stages },
        jobError: null,
        warnings: [],
      });
      try {
        const id = await api.startGenerate({
          videoPath: media.path,
          audioStreamIndex: audioStream,
          durationMs: media.durationMs,
          spokenLanguage: settings.spokenLanguage,
          captionLanguage: caption,
          transcribeModelId: settings.transcribeModelId ?? DEFAULT_MODEL_ID,
          translateModelId: settings.translateModelId,
          useGpu: settings.useGpu,
          maxLineChars: settings.maxLineChars,
        });
        set((st) => (st.job ? { job: { ...st.job, id } } : {}));
      } catch (e) {
        set({ job: null, jobError: toAppError(e) });
      }
    },

    async startTranslate(target) {
      const { project, history, settings, audioStream } = get();
      if (!project || !settings) return;
      const source = history.present.find((t) => t.kind === "original") ?? history.present[0];
      if (!source) return;
      set({
        job: {
          id: null,
          kind: "translate",
          stage: "preparing",
          progress: null,
          startedAt: Date.now(),
          stageStartedAt: Date.now(),
          stages: ["translating"],
          targetLanguage: target,
        },
        jobError: null,
      });
      try {
        const id = await api.startTranslate({
          videoPath: project.video.path,
          audioStreamIndex: audioStream ?? project.video.audioStreamIndex ?? 0,
          durationMs: project.video.durationMs,
          sourceTrack: source,
          targetLanguage: target,
          translateModelId: settings.translateModelId,
          useGpu: settings.useGpu,
          maxLineChars: settings.maxLineChars,
        });
        set((st) => (st.job ? { job: { ...st.job, id } } : {}));
      } catch (e) {
        set({ job: null, jobError: toAppError(e) });
      }
    },

    async cancelJob() {
      const job = get().job;
      if (job?.id) await api.cancelJob(job.id);
      else set({ job: null });
    },

    dismissJobError: () => set({ jobError: null }),

    onJobProgress(e) {
      if (e.kind === "preview") {
        const p = get().preview;
        if (p.status === "converting" && (!p.jobId || p.jobId === e.jobId)) {
          set({ preview: { ...p, jobId: e.jobId, progress: e.progress ?? p.progress } });
        }
        return;
      }
      const job = get().job;
      if (!job || (job.id && job.id !== e.jobId)) return;
      const stages = job.stages.includes(e.stage) || e.stage === "preparing" || e.stage === "finishing" ? job.stages : [...job.stages, e.stage];
      const stageStartedAt = e.stage !== job.stage ? Date.now() : job.stageStartedAt;
      set({ job: { ...job, id: e.jobId, stage: e.stage, progress: e.progress, detail: e.detail, stages, stageStartedAt } });
    },

    onJobFinished(e) {
      if (e.kind === "preview") {
        const p = get().preview;
        if (p.jobId && p.jobId !== e.jobId) return;
        if (e.outcome.status === "success") {
          const r = e.outcome.result as Preview;
          set({ preview: { status: r.hasVideo ? "ready" : "audio-only", url: mediaUrl(r.path) } });
        } else if (e.outcome.status === "failed") {
          set({ preview: { status: "failed", url: null, error: e.outcome.error } });
        } else {
          set({ preview: { status: "original", url: get().media ? mediaUrl(get().media!.path) : null } });
        }
        return;
      }
      const job = get().job;
      if (!job || (job.id && job.id !== e.jobId)) return;
      if (e.outcome.status === "cancelled") {
        set({ job: null });
        return;
      }
      if (e.outcome.status === "failed") {
        set({ job: null, jobError: e.outcome.error });
        return;
      }
      const result = e.outcome.result as GenerateResult;
      if (job.kind === "translate") {
        const { history } = get();
        // Replace an existing track in the same language rather than piling up copies.
        const langs = new Set(result.tracks.map((t) => t.language));
        const kept = history.present.filter((t) => t.kind === "original" || !langs.has(t.language));
        set({
          history: record(history, [...kept, ...result.tracks]),
          activeTrackId: result.activeTrackId,
          job: null,
          dirty: true,
          selectedCueId: null,
        });
        return;
      }
      const { media, settings, audioStream } = get();
      if (!media || !settings) return;
      const now = Date.now();
      const project: Project = {
        format: "captionnest.project",
        version: 1,
        id: ops.newId(),
        createdAtMs: now,
        updatedAtMs: now,
        video: {
          path: media.path,
          fileName: media.fileName,
          sizeBytes: media.sizeBytes,
          durationMs: media.durationMs,
          audioStreamIndex: audioStream,
        },
        settings: {
          spokenLanguage: settings.spokenLanguage,
          detectedLanguage: result.detectedLanguage,
          captionLanguage: settings.captionLanguage,
          modelId: settings.transcribeModelId ?? DEFAULT_MODEL_ID,
        },
        tracks: result.tracks,
        activeTrackId: result.activeTrackId,
      };
      set({
        project,
        projectPath: null,
        history: createHistory(result.tracks),
        activeTrackId: result.activeTrackId,
        selectedCueId: null,
        dirty: true,
        job: null,
        warnings: result.warnings,
        view: "workspace",
      });
    },

    async loadProject(path) {
      const loaded = await api.loadProject(path);
      const p = loaded.project;
      let media: MediaInfo | null = null;
      let videoMissing = loaded.videoStatus.status === "missing";
      if (loaded.videoStatus.status !== "missing") {
        try {
          media = await api.probeMedia(loaded.videoStatus.path);
        } catch {
          videoMissing = true;
        }
      }
      const project: Project = media ? { ...p, video: { ...p.video, path: media.path } } : p;
      set({
        project,
        projectPath: path,
        media,
        audioStream: p.video.audioStreamIndex,
        preview: { status: "original", url: media ? mediaUrl(media.path) : null },
        history: createHistory(p.tracks),
        activeTrackId: p.activeTrackId ?? p.tracks[0]?.id ?? null,
        selectedCueId: null,
        dirty: false,
        videoMissing,
        job: null,
        jobError: null,
        warnings: [],
        view: "workspace",
      });
      const s = get().settings;
      if (s) set({ settings: { ...s, recentProjects: (await api.getSettings()).recentProjects } });
    },

    async saveProject(path) {
      const { project, history, activeTrackId } = get();
      if (!project) return;
      const next: Project = { ...project, tracks: history.present, activeTrackId, updatedAtMs: Date.now() };
      await api.saveProject(path, next);
      set({ project: next, projectPath: path, dirty: false });
      const s = get().settings;
      if (s) set({ settings: { ...s, recentProjects: (await api.getSettings()).recentProjects } });
    },

    async relinkVideo(path) {
      const { project } = get();
      if (!project) return { ok: false };
      const media = await api.probeMedia(path);
      const mismatch = Math.abs(media.durationMs - project.video.durationMs) > 1500;
      set({
        media,
        project: {
          ...project,
          video: { ...project.video, path: media.path, fileName: media.fileName, sizeBytes: media.sizeBytes },
        },
        preview: { status: "original", url: mediaUrl(media.path) },
        videoMissing: false,
        dirty: true,
      });
      return { ok: true, durationMismatch: mismatch };
    },

    closeProject() {
      set({
        project: null,
        projectPath: null,
        media: null,
        dirty: false,
        videoMissing: false,
        history: createHistory<Track[]>([]),
        activeTrackId: null,
        selectedCueId: null,
        job: null,
        jobError: null,
        view: "home",
      });
    },

    backToOptions() {
      set({
        project: null,
        projectPath: null,
        dirty: false,
        history: createHistory<Track[]>([]),
        activeTrackId: null,
        selectedCueId: null,
        jobError: null,
        warnings: [],
        view: "prepare",
      });
    },

    setActiveTrack: (id) => set({ activeTrackId: id, selectedCueId: null }),
    selectCue: (id) => set({ selectedCueId: id }),
    setCueText: (id, text) => commit((c) => ops.updateCue(c, id, { text }), `text:${id}`),
    markChecked: (id) => commit((c) => ops.updateCue(c, id, { check: [] })),
    retimeCue: (id, s, e) => commit((c) => ops.retimeCue(c, id, s, e), `time:${id}`),
    addCue(atMs) {
      let newId: string | null = null;
      const max = get().project?.video.durationMs ?? Infinity;
      commit((c) => {
        const r = ops.addCue(c, atMs, 2000, max);
        newId = r.id;
        return r.cues;
      });
      if (newId) set({ selectedCueId: newId });
      return newId;
    },
    deleteCue: (id) => commit((c) => ops.deleteCue(c, id)),
    splitCue: (id, opts) => commit((c) => ops.splitCue(c, id, opts)),
    mergeCue: (id) => commit((c) => ops.mergeWithNext(c, id)),
    offsetAll: (delta) => commit((c) => ops.offsetAll(c, delta)),
    replaceCues: (cues) => commit(() => cues),
    deleteTrack(id) {
      const { history, activeTrackId } = get();
      const remaining = history.present.filter((t) => t.id !== id);
      if (remaining.length === 0) return;
      set({
        history: record(history, remaining),
        activeTrackId: activeTrackId === id ? remaining[0].id : activeTrackId,
        dirty: true,
      });
    },
    undo: () => set((st) => ({ history: undoH(st.history), dirty: true })),
    redo: () => set((st) => ({ history: redoH(st.history), dirty: true })),
  };
});

/** Cues of the track being edited. */
export const useActiveTrack = () =>
  useStore((s) => s.history.present.find((t) => t.id === s.activeTrackId) ?? null);

export const suggestedName = (fileName: string) => fileName.replace(/\.[^.]+$/, "");

export const installedModelIds = (models: ModelState | null) =>
  new Set((models?.whisper ?? []).filter((m) => m.installed).map((m) => m.id));

export const canTranslateInstalled = (models: ModelState | null) =>
  (models?.whisper ?? []).some((m) => m.installed && (MODEL_INFO[m.id]?.translates ?? m.canTranslate));
