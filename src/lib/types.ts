// Shapes shared with the Rust backend (serde camelCase).

export interface Cue {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  /** Words the recognizer was unsure about; shown for review, never exported. */
  check?: string[];
  /** Auto-sync couldn't confirm this caption's time; shown for review, never exported. */
  timingCheck?: boolean;
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
  | "online_failed"
  | "voice_key"
  | "voice_credits"
  | "voice_failed"
  | "font_invalid"
  | "link_failed";

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

export type JobKind = "generate" | "preview" | "burn" | "sync" | "link";

/** Auto-sync: one caption's new and old times. */
export interface SyncedCue {
  id: string;
  startMs: number;
  endMs: number;
  oldStartMs: number;
  oldEndMs: number;
  /** matched: heard in the audio. estimated: placed by its neighbours. unmatched: left as it was. outside: not in the synced section. */
  status: "matched" | "estimated" | "unmatched" | "outside";
  confidence: number;
}

export interface SyncSummary {
  kind: "in_sync" | "offset" | "drift" | "mixed" | "no_match";
  matched: number;
  estimated: number;
  unmatched: number;
  changed: number;
  medianShiftMs: number;
  firstShiftMs: number;
  lastShiftMs: number;
  maxShiftMs: number;
}

export interface SyncResult {
  cues: SyncedCue[];
  summary: SyncSummary;
  modelUsed: string | null;
}
export type Stage = "preparing" | "extracting" | "transcribing" | "translating" | "finishing" | "converting" | "rendering" | "downloading";

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
  /** Use online recognition when a key is set. */
  onlineRecognition: boolean;
  /** With both keys saved: "both" (Sarvam writes, Deepgram times), "sarvam" or "deepgram" (lib/online.ts). */
  onlineEngine: string;
  /** "sk_ab…9lnQ" when a Sarvam key is saved (the key itself stays in Rust). */
  sarvamKeyHint?: string | null;
  /** Interface theme id (lib/theme.ts). */
  theme: string;
  /** Where videos from links are saved (null: Movies/Videos › CaptionNest). */
  clipFolder?: string | null;
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
  /** Test build with the automation bridge. */
  automation: boolean;
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

/** A caption font the user added (one family, one or more files). */
export interface UserFontFace {
  file: string;
  weight: number;
  /** Upper end of the weight range for variable fonts. */
  weightMax?: number;
  italic: boolean;
  /** Whether the font has Bengali letters (null when unknown). */
  bangla: boolean | null;
}

export interface UserFont {
  id: string;
  name: string;
  faces: UserFontFace[];
}

export interface FontImportResult {
  fonts: UserFont[];
  added: string[];
  errors: { file: string; message: string }[];
}

/** Voice typing: whether a Deepgram key is saved (the key itself never reaches the UI). */
export interface VoiceStatus {
  keySaved: boolean;
  keyHint: string | null;
}

export interface Heard {
  text: string;
  confidence: number;
}

/** Live voice typing (Rust: voice_live::LiveEvent). */
export interface VoiceLiveEvent {
  id: number;
  /** "words": a running guess, or final words when `isFinal`; "end": nothing more comes. */
  kind: "words" | "end" | "error";
  text: string;
  isFinal: boolean;
  error: AppError | null;
}

// ---------- video links ----------

export interface LinkToolsState {
  installed: boolean;
  /** yt-dlp's version, e.g. "2026.11.23". */
  version: string | null;
  /** About how much the first download is. */
  downloadBytes: number;
  defaultFolder: string;
}

export type LinkQuality = "editing" | "best" | "audio";

export interface QualityChoice {
  quality: LinkQuality;
  available: boolean;
  height: number | null;
  ext: string | null;
  /** Video codec family: "avc1" is H.264 (opens in every editor), "vp09", "av01"… */
  vcodec: string | null;
  wholeBytes: number | null;
  partBytesPerSec: number | null;
}

/** YouTube's preview frames: sprite sheets of `columns` × `rows` frames. */
export interface Storyboard {
  sheets: string[];
  columns: number;
  rows: number;
  width: number;
  height: number;
  frameMs: number;
}

export interface LinkInfo {
  key: string;
  url: string;
  title: string;
  uploader: string | null;
  site: string;
  durationMs: number | null;
  /** Where the link asked to start (YouTube's t=). */
  startMs: number | null;
  thumbnail: string | null;
  storyboard: Storyboard | null;
  choices: QualityChoice[];
}

export interface LinkRequest {
  key: string;
  url: string;
  quality: LinkQuality;
  fromMs: number | null;
  toMs: number | null;
  folder: string;
  estimateBytes: number | null;
}

export interface LinkResult {
  path: string;
  bytes: number;
}
