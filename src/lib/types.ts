// Shapes shared with the Rust backend (serde camelCase).

export interface Cue {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  /** Words the recognizer was unsure about; shown for review, never exported. */
  check?: string[];
}

export type TrackKind = "original" | "translation";

export interface Track {
  id: string;
  kind: TrackKind;
  language: string;
  engine: "whisper" | "whisper-translate" | "argos" | string;
  sourceTrackId?: string | null;
  cues: Cue[];
}

export interface VideoRef {
  path: string;
  relativePath?: string | null;
  fileName: string;
  sizeBytes: number;
  durationMs: number;
  audioStreamIndex: number | null;
}

export interface GenerationSettings {
  spokenLanguage: string;
  detectedLanguage?: string | null;
  captionLanguage: string;
  modelId: string;
}

export interface Project {
  format: string;
  version: number;
  id: string;
  createdAtMs: number;
  updatedAtMs: number;
  video: VideoRef;
  settings: GenerationSettings;
  tracks: Track[];
  activeTrackId: string | null;
  /** Caption look for the preview and burned-in exports (see captionStyle.ts). */
  style?: unknown;
}

export type VideoStatus =
  | { status: "found"; path: string }
  | { status: "changed"; path: string }
  | { status: "missing" };

export interface LoadedProject {
  project: Project;
  videoStatus: VideoStatus;
}

export interface AudioTrack {
  streamIndex: number;
  codec: string;
  channels: number;
  sampleRate: number;
  language?: string | null;
  title?: string | null;
  isDefault: boolean;
  durationMs?: number | null;
}

export interface VideoStream {
  codec: string;
  width: number;
  height: number;
  frameRate?: number | null;
  variableFrameRate: boolean;
  /** Clockwise rotation applied on playback: 0, 90, 180 or 270. */
  rotation: number;
  bitRate?: number | null;
}

export interface MediaInfo {
  path: string;
  fileName: string;
  sizeBytes: number;
  durationMs: number;
  container: string;
  video: VideoStream | null;
  audioTracks: AudioTrack[];
  defaultAudio: number | null;
  previewLikelyPlayable: boolean;
}

export type ErrorCode =
  | "file_not_found"
  | "no_audio"
  | "corrupt_media"
  | "unsupported_media"
  | "no_speech"
  | "disk_full"
  | "model_missing"
  | "download_failed"
  | "checksum_mismatch"
  | "network"
  | "cancelled"
  | "tool_missing"
  | "tool_failed"
  | "translation_unavailable"
  | "translation_failed"
  | "project_invalid"
  | "project_too_new"
  | "export_invalid"
  | "io"
  | "internal"
  | "online_key"
  | "online_credits"
  | "online_failed";

export interface AppError {
  code: ErrorCode;
  message: string;
  detail?: string;
}

export interface WhisperModelEntry {
  id: string;
  file: string;
  url: string;
  sizeBytes: number;
  sha256: string;
  canTranslate: boolean;
  role: string;
  /** Set for models trained for one language; used automatically for that language. */
  language?: string | null;
  installed: boolean;
  downloading: boolean;
  partialBytes: number;
}

export interface ArgosEntry {
  id: string;
  from: string;
  to: string;
  fromName?: string | null;
  toName?: string | null;
  version?: string | null;
  sizeBytes: number;
  installed: boolean;
  downloading: boolean;
}

export interface ModelState {
  whisper: WhisperModelEntry[];
  argos: ArgosEntry[];
  diskUsageBytes: number;
  freeBytes: number | null;
  modelsDir: string;
}

export interface DownloadEvent {
  id: string;
  state: "downloading" | "verifying" | "installing" | "done" | "failed" | "cancelled";
  received: number;
  total: number;
  bytesPerSec: number;
  error?: AppError;
}

export type JobKind = "generate" | "preview" | "burn";
export type Stage = "preparing" | "extracting" | "transcribing" | "translating" | "finishing" | "converting" | "rendering";

export interface ProgressEvent {
  jobId: string;
  kind: JobKind;
  stage: Stage;
  progress: number | null;
  detail?: string;
}

export type Outcome<T> =
  | { status: "success"; result: T }
  | { status: "failed"; error: AppError }
  | { status: "cancelled" };

export interface FinishedEvent<T> {
  jobId: string;
  kind: JobKind;
  outcome: Outcome<T>;
}

export interface GenerateResult {
  tracks: Track[];
  activeTrackId: string;
  detectedLanguage: string | null;
  usedGpu: boolean;
  warnings: string[];
}

export interface Preview {
  path: string;
  hasVideo: boolean;
}

export interface Settings {
  useGpu: boolean;
  transcribeModelId: string | null;
  translateModelId: string | null;
  maxLineChars: number;
  spokenLanguage: string;
  captionLanguage: string;
  recentProjects: RecentProject[];
  setupComplete: boolean;
  /** Use Sarvam online recognition when a key is set. */
  onlineRecognition: boolean;
  sarvamApiKey?: string | null;
}

export interface RecentProject {
  path: string;
  name: string;
  videoName: string;
  openedAtMs: number;
}

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
  gpuBackend: string | null;
  modelsDir: string;
  logDir: string | null;
}

export type IssueKind = "empty_text" | "negative_time" | "invalid_duration" | "out_of_order" | "overlap";

export interface ValidationIssue {
  kind: IssueKind;
  cueId: string;
  otherCueId?: string;
  position: number;
  blocking: boolean;
  overlapMs?: number;
}

export interface ValidationReport {
  issues: ValidationIssue[];
  cueCount: number;
}

export type Plan =
  | { kind: "same" }
  | { kind: "whisperEnglish" }
  | { kind: "text"; fromWhisperEnglish: boolean; packages: string[] };

export interface PlanInfo {
  plan: Plan | null;
  missingPackages: string[];
  needsTranslationModel: boolean;
}
