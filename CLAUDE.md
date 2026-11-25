# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

CaptionNest is a Tauri 2 desktop app (Windows x64, macOS Apple Silicon and Intel) for
non-technical creators: video → speech recognition → optional translation → caption
editor → SRT export for CapCut, or an MP4 with captions burned in. The React 19 +
TypeScript interface lives in `src/`, the Rust backend in `src-tauri/`. Recognition runs
offline with a bundled whisper.cpp, or online with Sarvam AI when the user has saved an
API key. Translation runs Argos Translate packages through CTranslate2 (`ct2rs`), with no
Python.

## Commands

```sh
npm ci
npm run typecheck                               # tsc strict; this is the only "lint"
npm test                                        # vitest, src/**/*.test.ts
npx vitest run src/lib/cueOps.test.ts           # one test file
npx vitest run -t "merge"                       # tests whose name matches
(cd src-tauri && cargo test --lib)              # Rust unit tests (inline #[cfg(test)] modules)
(cd src-tauri && cargo test --lib asr::segment) # one module or test-name filter
npm run tauri dev
npx tauri build --target aarch64-apple-darwin   # x86_64-apple-darwin for Intel; plain `npx tauri build` on Windows
```

CI (`.github/workflows/build.yml`) runs typecheck, `npm test`, `cargo test --lib`,
`tauri build`, then installs each package and runs a speech smoke test on it. Pushing a
`v*` tag creates a **draft** release.

### Bundled tools must be staged first

The app never uses tools from PATH. `ffmpeg`, `ffprobe`, `whisper-cli` and `whisper-vad`
are built from pinned sources and copied into `src-tauri/resources/tools/` (gitignored).
Do this before `tauri dev` or `tauri build`:

```sh
scripts/tools/build-ffmpeg-macos.sh arm64        # or x86_64
scripts/tools/build-whisper-macos.sh arm64
scripts/tools/stage-tools.sh aarch64-apple-darwin
```

Versions and SHA-256 hashes are in `scripts/tools/versions.env`. The Windows steps are in
`docs/BUILD.md`. The first Rust build also compiles CTranslate2 (about 10 minutes).

### Environment variables

- `CAPTIONNEST_HOME=<dir>` puts all models, settings and caches in one folder (clean-state testing).
- `CAPTIONNEST_AUTOMATION_PORT=7788` together with `--features automation` turns on the UI automation bridge.
- `CAPTIONNEST_ARGOS_ROOT=<installed package dir>` enables the ignored Argos reference test:
  `cargo test --lib matches_reference -- --ignored`.
- `SARVAM_API_KEY` (environment or the gitignored `.env`) is used by `--self-test --online` and `scripts/dev/online_check.py`;
  `DEEPGRAM_API_KEY` adds Deepgram to `--self-test --online` (`--engine both|sarvam|deepgram`, as in Settings).
- `CAPTIONNEST_DEBUG_DIR=<dir>` makes the pipeline write what it heard and where it put it
  (`online-timing.json`, `resync-heard.json`, `resync-regions.json`), for timing work.
  Measurement-only switches: `CAPTIONNEST_WORD_TIMING=0|sparse|all`,
  `CAPTIONNEST_JOIN_PAUSE_MS`, `CAPTIONNEST_SYNC_MODE=greedy|beam[-dtw]`, `CAPTIONNEST_NO_AC`,
  `CAPTIONNEST_DG_SNAP_MS` (how far a Deepgram-timed caption start may move to a speech onset).
- `DEEPGRAM_API_KEY` plus `CAPTIONNEST_VOICE_SAMPLES=<a.wav,b.webm,…>` enable the ignored live voice-typing test:
  `cargo test --lib live_bangla_transcription -- --ignored --nocapture`.

### Headless self-test

`<app binary> --self-test <media> --model <ggml.bin> --out out.srt [--language xx] [--online]`
runs the real pipeline with the bundled tools and no window. It writes a JSON report to
`out.json` and exits 0 on success. With `--resync <captions.srt> [--from s] [--to s]` it
auto-syncs those captions to the media instead (using the models installed under
`CAPTIONNEST_HOME`) and writes the moved captions plus a per-caption report. `scripts/ci/smoke-macos.sh` and `smoke-windows.ps1`
use it on installed packages.

### Driving the real UI

```sh
CAPTIONNEST_AUTOMATION_PORT=7788 npm run tauri dev -- --features automation
python3 scripts/dev/ui.py shot build/shots/home.png     # snapshot (macOS only)
python3 scripts/dev/ui.py eval 'return document.title'  # JS in the real web view
python3 scripts/dev/flow.py generate build/testmedia/<video>.mp4 --spoken bn --caption en --shots demo
python3 scripts/dev/verify_suite.py                     # checks behind docs/VERIFICATION.md
python3 scripts/dev/ui_audit.py <label>                 # every screen at 1360x860 and 900x600: layout checks + snapshots
```

In builds with the `automation` feature (and only there), the zustand store is exposed as
`window.__CN_STORE__` and `window.__CN_AUTOMATION__` stands in for file dialogs. The
bridge refuses requests that carry browser headers (Origin, Referer, Sec-Fetch-*) or a
non-loopback Host, so a web page can't drive the app.
`scripts/dev/make-test-media.sh` and `make_long_video.py` create test videos with known
speech. The automation feature never ships in release builds.

Notes on automation:

- With `CAPTIONNEST_AUTOMATION_PORT` set, an automation build skips the single-instance
  guard, so it can run next to an installed copy. Use `CAPTIONNEST_HOME` to keep its data apart.
- A window that is covered by other windows doesn't run CSS animations, so screens stay
  at opacity 0 in snapshots. Run `document.getAnimations().forEach(a => a.finish())` before
  taking a snapshot.
- `/snapshot` paths are resolved from the app's working directory. Pass absolute paths.

## Architecture

### Frontend ↔ backend contract

- `src/lib/api.ts` is the only place that calls `invoke`. Each wrapper maps to a
  `#[tauri::command]` in `src-tauri/src/lib.rs`, and the command must also be listed in
  `generate_handler!`.
- `src/lib/types.ts` mirrors the Rust structs by hand. Serde uses `rename_all = "camelCase"`
  for structs and `snake_case` for enums. There is no code generation, so change both sides together.
- Errors: Rust `AppError { code, message, detail }` (`src-tauri/src/error.rs`). `code` is a
  stable snake_case `ErrorCode`. `src/lib/errors.ts` maps each code to a title and recovery
  actions, so a new `ErrorCode` needs a case there. `message` is plain language shown to the
  user; technical details go in `detail` (shown behind "Show details").
- Long work runs as jobs (`src-tauri/src/jobs.rs`). A `start_*` command returns a job id at
  once and spawns a task. Progress arrives as `job-progress` events, the result as one
  `job-finished` event (`success` / `failed` / `cancelled`). Only one job per `JobKind`
  (`Generate`, `Preview`, `Burn`, `Sync`) runs at a time: starting a new one cancels the old
  one, and an outdated job's result is discarded. Translation reuses `JobKind::Generate`.
  `Burn` and `Sync` results go to their own small stores (`state/burn.ts`, `state/sync.ts`),
  not to the main store's `job`.
  Model downloads are tracked separately by model id and emit `download-progress`.
- Each job gets a `ScratchDir` that is deleted when the job ends; `sweep_abandoned` cleans
  leftovers at startup.
- Wrap a job's work in `jobs::guard(...)` so a panic still ends the job with an error;
  otherwise the interface waits forever for `job-finished`.
- Plain (non-async) commands run on the main thread. Any command that touches the disk
  must be `async` and do its work through `off_main` in `lib.rs`, or a slow drive
  freezes the window.

### Generation pipeline (`src-tauri/src/pipeline.rs`)

1. Probe with ffprobe, then extract 16 kHz mono WAV that keeps the original video timeline
   (`media/extract.rs`). WAV slices are read from disk, so long videos never load into memory.
2. Voice activity detection with the bundled Silero model as a separate step
   (`asr/vad.rs`, the `whisper-vad` tool), not whisper-cli's built-in `--vad`.
3. Language detection (when "auto") votes at up to three points in the speech and prefers
   the Accurate model `large-v3-q5_0` when it is installed.
4. Recognition in speech chunks of at most 12 s through `whisper-cli` (`asr/whisper.rs`).
   Sarvam (`asr/sarvam.rs`) is used instead when a key is set and it supports the spoken
   language; with a Deepgram key too (the voice-typing key), Deepgram (`asr/deepgram.rs`)
   listens to the same chunks and times Sarvam's words. Deepgram writes the captions on its
   own where Sarvam can't (no key, no credits, language) or when chosen in Settings
   (`Settings.online_engine`: `both | sarvam | deepgram`). A language-specialised model
   (Bangla) is used automatically when installed.
5. Bengali script repair (`asr/script.rs`) and language prompts for punctuation (`asr/prompt.rs`).
6. Word timings become cues (`asr/segment.rs`), with cue edges snapped to VAD speech
   onsets and offsets.
7. Optional translation. `translate/routes.rs` picks one route: none, Whisper
   speech-to-English, or Argos text translation that keeps every cue's timing (pivoting
   through English when needed).

### Caption timing (read before touching it)

Three measured facts shape all timing code:

- **Pauses are exact, word times are not.** The VAD's speech regions come from the sound
  itself. whisper.cpp's word times, plain or with `-dtw`, greedy or beam, were 1-2 s off
  in about 15% of captions against exactly timed speech
  (`scripts/bench/make_phrase_audio.py` builds that recording; `resync_check.py` scores
  against it). It smears words over audio it skipped and slides words back across a pause.
- **Sarvam returns one time span per request**, not per word. So every request is one
  stretch of speech between pauses, at most 12 s (`vad::plan_pause_chunks`), which pins its
  text. Captions from online text never run across a pause of 0.5 s or more. Where the
  voice fills only part of a chunk, `asr/align.rs` uses an offline pass to find where.
- **Deepgram times every word**, and its times hold up under music where pauses found from
  the sound don't. With a Deepgram key, its words of the same chunks replace the offline
  pass (`align::Timing::Exact`: no slowness check, which threw away the first word after a
  pause). But it writes Bangla worse (16.0% vs 13.1% word errors) and skipped 14 s of
  talking under music on a real vlog, so Sarvam keeps writing the text.
- **Text is never altered by timing code.** Online words go into captions unchanged
  (`segment::Options::keep_all_words`), with a check that no letter was lost.

Auto-sync (`asr/resync.rs`, `pipeline::resync_captions`, `JobKind::Sync`) moves existing
captions to their speech and returns new times only. It matches caption words to heard
words in order (fuzzy; Bengali and Latin script compared by sound), uses that only to
learn which stretch of speech each word is in, and lays the words out by length between
pauses. Captions it can't hear are never guessed: they are placed by the layout or their
neighbours and get `Cue.timing_check` ("Check timing" in the editor), or stay where they
were. A caption that begins after a pause lands on it exactly; one that begins mid-speech
is good to about 0.3-0.45 s. Re-measure with the scripts above after any change here;
`scripts/bench/cue_window_check.py` scores any SRT against real video without ground
truth (it costs Sarvam credits).

Speech that can't be recognized is kept as an empty, flagged cue; nothing is dropped
silently. Low-confidence words go into `Cue.check`: the editor shows them as "Check"
chips, and they are never exported.

Most of these choices (chunk size, models, prompts, separate VAD, detection model) come
from benchmarks in `scripts/bench/`. The numbers are recorded in `docs/VERIFICATION.md` and
in each module's `//!` doc comment. Read those before you change recognition behaviour,
and re-measure instead of guessing.

### Tools, paths and models

- `Paths::tool(name)` (`src-tauri/src/paths.rs`) resolves bundled tools only from the
  app's resource folder. `media/process.rs` runs them with a hidden console on Windows,
  kills them on cancel and streams their output.
- Model catalogs `src-tauri/catalog/whisper.json` and `argos.json` are compiled into the
  app with URL, size and SHA-256. Downloads resume from `<file>.part` with HTTP Range and
  are SHA-256 checked before the rename (`models/download.rs`). User-facing names and
  recommendations for speech models live in `src/lib/models.ts`, keyed by catalog id.
  Rebuild the Argos catalog with `python3 scripts/catalog/build_argos_catalog.py`.

### Captions, projects, export

- `captions/srt.rs` writes SRT for CapCut: UTF-8 without BOM, CRLF line endings.
  `captions/validate.rs` separates blocking problems (export refused with `ExportInvalid`)
  from overlaps (reported so the user can choose a fix).
- `project.rs`: `.captionnest` files are versioned JSON. They reference the video (never
  copy it) and are written atomically (temp file, fsync, rename via `project::persist`).
  A format change needs a `CURRENT_VERSION` bump and a step in `migrate`. Newer files
  fail with `ProjectTooNew`.
- Burned-in video export: `src/lib/renderCaption.ts` draws each caption as a full-frame
  PNG. It is the same renderer the live preview uses (`components/CaptionCanvas.tsx`).
  `src/state/burn.ts` sends the PNGs as raw bytes through `burn_frame`, then
  `media/burn.rs` has FFmpeg concat and overlay them. Caption appearance changes belong in
  `captionStyle.ts` / `renderCaption.ts` (not CSS), so the preview and the export stay identical.

### Frontend

- One main zustand store, `src/state/store.ts`: the view (`loading | setup | home | prepare
  | workspace`), settings, models and downloads, media, project, undo history, the current
  job and open panels. Playback position is in its own store (`state/playhead.ts`) because
  it changes every frame. Pure caption edits are in `lib/cueOps.ts` and `lib/history.ts`,
  which are unit-tested; the store calls them.
- Screens are in `src/screens/`, modal panels in `src/panels/`, shared primitives in
  `src/components/ui.tsx` (Radix through the `radix-ui` package, fully restyled).
  Styling is plain CSS with design tokens in `src/styles/tokens.css` (no Tailwind).
  Icons come from Phosphor only; toasts from sonner.
- Themes (Settings > Appearance; `Settings.theme`, ids in `lib/theme.ts`): each theme sets
  only base colours under `[data-theme="…"]`; soft and see-through shades are
  `color-mix()`ed from them, so never write colour literals in component CSS (use
  `var(--…)`; gold words use `--gold-text`, which is darker on Daylight). What stands for
  the video (player, caption previews, style swatches) keeps fixed colours in every theme.
  `src-tauri/src/theme.rs` matches the native window (light/dark chrome, launch colour).
  Check theme work with `scripts/dev/ui_audit.py <label> <theme>`.
- The CSP in `src-tauri/tauri.conf.json` allows only IPC for `connect-src`, and fonts are
  bundled through `@fontsource`. The web view can't reach the network, so all network
  access (model downloads, Sarvam, Deepgram) happens in Rust. API keys never reach the
  web view: `get_settings` returns a masked `sarvamKeyHint`, and keys change only through
  `save_online_key` / `remove_online_key` and `save_voice_key` / `remove_voice_key`.
  `opener:allow-open-url` lists the exact sites the app links to. New plugin APIs also need a permission
  in `src-tauri/capabilities/default.json`.
- The caption list (`components/CueList.tsx`) is virtualized with @tanstack/react-virtual.
  Rows stack in normal flow inside one block shifted by the first item's start, so a
  caption that grows can't overlap the next one while measurement catches up. Keep it
  that way; absolutely positioned rows caused overlapping captions.

### Typing aids and added fonts

- **Bangla phonetic typing**: `src/lib/phonetic.ts` is a pure, unit-tested converter
  (Avro conventions). It searches ambiguous spellings with a cost per less likely choice
  and ranks them by a vocabulary: the project's Bangla captions plus
  `lib/banglaWords.ts`. `components/usePhonetic.ts` wires it into the caption textarea.
  It keeps the roman letters of the word being typed and re-spells the whole word on
  every key.
- **Voice typing** is live: `src/state/voice.ts` turns the microphone into 16 kHz 16-bit
  PCM (a ScriptProcessor on an `AudioContext`) and sends ~128 ms pieces, numbered, to
  `voice_live_audio`; `src-tauri/src/voice_live.rs` streams them over a WebSocket to
  Deepgram (`interim_results`, `endpointing=300`, KeepAlive) and emits `voice-live` events.
  The running guess and final words are written into the caption at the cursor as you speak
  (`lib/insertWords.ts` from the caption text at the start; Cancel takes them out; editing the
  caption meanwhile ends dictation). The whole recording is kept with `MediaRecorder` too:
  if the live connection fails, it goes to `transcribe_voice` (Deepgram pre-recorded,
  `voice.rs`) after Stop. The Deepgram key lives in `config/voice.json`, separate from
  `settings.json`, and never reaches the web view.
- WKWebView offers `navigator.mediaDevices` only when the app's Info.plist has
  `NSMicrophoneUsageDescription` (`src-tauri/Info.plist`). Tauri embeds that file only in
  dev builds (no `custom-protocol` feature); bundles merge it. A bare
  `cargo build --features tauri/custom-protocol` binary therefore has no microphone API.
  Test voice typing with `tauri dev` or a bundled `.app`. `Entitlements.plist` adds
  audio input for the hardened runtime.
- **Added caption fonts**: `src-tauri/src/fonts.rs` copies files into `data/fonts/`.
  Faces are grouped by family, read from the font's `name`, `OS/2`, `fvar` and `cmap`
  tables (or from the file name for WOFF/WOFF2). Ids are `user-<sha of family>`, the
  same on every computer. `src/lib/userFonts.ts` loads each file as bytes into a
  `FontFace` (the CSP allows no font URLs) and registers it with `captionStyle.ts`.
  A project whose added font is missing keeps the id and draws with the default font.

## Conventions

- Users are non-technical. All UI text and error messages are plain language without
  jargon (say "speech model", not "ggml"). UI polish is a primary deliverable: check UI
  changes with snapshots of the real app through the automation bridge.
- Comments and docs explain *why*, with measured numbers where a choice came from a benchmark.
- FFmpeg must stay LGPL-only and unmodified. After dependency changes, run
  `node scripts/licenses/collect.mjs`: it rewrites `DEPENDENCIES.txt` and
  `THIRD_PARTY_LICENSES.txt` (the license texts and copyright notices that BSD, ISC,
  Zlib, MPL, Unicode and OFL packages require) in `src-tauri/resources/licenses/`.
- Pin downloads to fixed versions: Hugging Face `resolve/<commit>/` URLs, never
  `resolve/main/`, and GitHub Actions by commit SHA.
- Releases stay drafts until the maintainer publishes them.
