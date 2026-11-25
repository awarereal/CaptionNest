# CaptionNest — verification summary

What was tested, how, and what is still pending. Test machine unless noted: MacBook Pro
M3 Pro (18 GB), macOS 27.2.1. Scripts live in `scripts/bench`, `scripts/dev` and
`scripts/ci`; raw results in `build/bench/*.json` (not committed).

## Speech recognition accuracy (real Bangla speech)

Test set: 20 utterances (243 s) of real read Bangla speech from Google FLEURS (dev split,
CC-BY 4.0) with human transcripts. Scores ignore punctuation. **WER** counts every
misspelled word as wrong, so it is the spelling measure; **CER** counts characters.

| Model | WER | CER | Bengali script | Speed (× real time, M3 Pro GPU) |
|---|---|---|---|---|
| **Bangla** — Mozilla AI large-v3-turbo-bn (q5_0) | **36.4%** | **23.6%** | 100% | 0.34 |
| **Bangla Max** — Mozilla AI large-v3-bn (q5_0) | **35.0%** | **23.5%** | 100% | 1.35 |
| Accurate — OpenAI large-v3 (q5_0) | 73.6% | 40.9% | 89% | 1.23 |
| Fast — OpenAI large-v3-turbo (q5_0), with script repair | 86.0% | 44.4% | 63% → 100% | 0.25 |
| Light — OpenAI small | 110% | 104% | 0% | 0.18 |

Decisions that followed:
- The Bangla-trained models halve spelling errors, so Bangla speech is transcribed with
  them automatically when installed ("Bangla" recommended, "Bangla Max" optional).
- Small/Light can't write Bengali script at all; the app says so in the language picker.
- Language detection uses the Accurate model: it identified real Bangla 20/20 times; the
  Fast model only 3/20 (it guessed Gujarati or Hindi). Detection votes across three
  points in the video and warns when uncertain.

### Recognition strategy (same 39 s Bangla clip, Accurate model)

| Approach | CER |
|---|---|
| Whole file in one pass | 54.9% (skipped a full sentence) |
| Whisper's built-in VAD | 54.5% (Fast model stopped after one sentence) |
| Speech chunks ≤ 18 s | 40.4% |
| **Speech chunks ≤ 12 s (shipped)** | **24.0%**, nothing skipped |

Other measured changes: punctuated language prompts restore punctuation and capitals
lost in timestamped decoding; a chunk-sized encoder window (`-ac`) was 22% faster with
CER 27.5% vs 28.8%; two parallel recognizer processes were ~19% faster.

## Long video

30.2-minute video of 126 real Bangla utterances with 0.5–3 s pauses, run through the real
app UI (Accurate model, before the Bangla model existed):
- **0 of 126 utterances without a caption**, 0 empty "needs review" captions
- 316 captions, all in order, 0 overlaps, last caption ends 0.6 s before speech ends
- CER 28.8% (one-pass Whisper scored 41–55% on comparable audio)
- ran at 1.32 × real time (with other benchmarks competing for the GPU)

## Automated tests

- Rust: 97 unit tests (`cargo test --lib`) — SRT timestamps/serialization/parsing,
  export validation and overlap fixing, project save/load/relink/atomicity, WAV slicing,
  VAD parsing and chunk planning, coverage gaps, Whisper JSON parsing (incl. invalid
  UTF-8 from real output), word confidence, segmentation (Bangla fixture), Bengali script
  repair, prompt-echo filtering, language voting, translation routes, resumable verified
  downloads (local HTTP server), job cancellation and scratch cleanup.
- Frontend: 22 Vitest tests — cue editing (split/merge/add/delete/offset), undo history,
  timecode parsing.
- CI (GitHub Actions) builds every platform from pinned sources, runs both test suites,
  installs the package and runs the bundled pipeline on synthesized speech.

## Real-app checks (driven through the interface)

Performed on the macOS Apple Silicon development build via the automation bridge:

| Check | Result |
|---|---|
| English speech → English captions (path with spaces and Bengali characters) | pass — punctuated, sentence-split |
| Bangla speech → Bangla captions | pass |
| Bangla speech → English captions (auto-detected Bangla) | pass |
| 30-minute video | pass (above) |
| Language uncertainty warning, unrecognized-speech review captions | implemented, unit-tested |

## Pending / limitations

See the end of the delivery report for the up-to-date list; it is kept in sync with this
file at release time.
