# CaptionNest

**Turn the speech in a video into accurate, editable captions — on your own computer — and
export an SRT file for CapCut.**

Add a video → choose the spoken and caption languages → generate → review and edit →
**Export SRT for CapCut** → import the original video and the SRT into CapCut, where every
caption becomes its own editable clip.

- **Offline and free.** Speech recognition (OpenAI Whisper via whisper.cpp) and translation
  (Argos Translate packages via CTranslate2) run locally. Models download once; no
  account, API key or subscription. Videos never leave the computer.
- **Built for Bangla, English, Hindi, Arabic and 90+ more languages**, with honest
  per-language quality guidance. English captions can be made from any spoken language;
  other caption languages use offline translation packs.
- **Nothing silently missed.** Speech is found with a voice-activity model and recognized
  in short chunks; stretches that can't be recognized are kept as flagged empty captions.
- **A real editor.** Synced preview with caption overlay, caption lane timeline, editable
  timecodes, split / merge / add / delete, timing shift, undo/redo, projects that survive
  moved videos, overlap and validity checks before export.
- **Bangla without a Bangla keyboard.** Type phonetically (`ami` → আমি) with spelling
  choices learned from your own captions, or dictate with voice typing (Deepgram Nova-3,
  optional, needs your key).
- **Your own fonts** for the preview and burned-in video exports (.ttf, .otf, .woff, .woff2).
- **Long videos.** Audio is processed in chunks from disk; the caption list is virtualized.

| Platform | Package |
|---|---|
| Windows 10/11 x64 | NSIS installer (per-user) |
| macOS 13.3+ Apple Silicon | `.dmg` |
| macOS 13.3+ Intel | `.dmg` |

Recipients: see **[docs/START-HERE.md](docs/START-HERE.md)**.
Building: see **[docs/BUILD.md](docs/BUILD.md)**. Test results and known limits:
**[docs/VERIFICATION.md](docs/VERIFICATION.md)**.

## Project layout

```
src/                 React + TypeScript interface (screens, components, state, styles)
src-tauri/src/       Rust backend
  media/             ffprobe inspection, audio extraction, WAV slicing, preview copies
  asr/               whisper-cli runner, voice activity detection, caption segmentation,
                     Bengali script repair, language prompts
  translate/         Argos packages on CTranslate2, translation route planning
  models/            pinned catalog, resumable SHA-256-verified downloads
  captions/          SRT time format, writer/reader, export validation
  project.rs         versioned .captionnest files with atomic saves
  pipeline.rs        generation and translation jobs
src-tauri/catalog/   pinned model catalogs (URLs, sizes, SHA-256)
scripts/tools/       reproducible FFmpeg / whisper.cpp builds from pinned sources
scripts/bench/       accuracy benchmarks behind the model and decoding choices
scripts/ci/          installed-package smoke tests
```

## License

CaptionNest is MIT-licensed. Bundled components keep their own licenses — notably FFmpeg
(LGPL v2.1+, shipped unmodified with its source) — see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
