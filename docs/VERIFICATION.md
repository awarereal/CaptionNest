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

The same models inside the app pipeline (speech chunks, language prompt, segmenting),
on 5 minutes of real FLEURS Bangla speech, 381 reference words:

| Model in CaptionNest | WER | CER | Speed (× real time) |
|---|---|---|---|
| **Bangla Max** | **26.5%** | **11.7%** | 0.91 |
| **Bangla** (with Bangla punctuation prompt) | 32.3% | 13.9% | 0.24 |
| Accurate (stock large-v3) | 75.6% | 26.4% | — |

**Online recognition (Sarvam AI `saaras:v4`)**, same 5-minute test, run the way the app
runs it (speech chunks up to 25 s): **13.1% WER, 4.0% CER**, 3 seconds for the whole
5 minutes. A real 6-minute Bangladeshi TV news bulletin (ATN Bangla) took 6.6 s online vs
102 s with the offline Bangla model, with names, greetings and conjuncts right where the
offline model garbled them. Sarvam writes money as "₹17,000"; CaptionNest rewrites it as
"১৭,০০০ টাকা" and uses Bengali digits in Bangla captions.

**Timing of online captions.** Sarvam gives one time span per request, so captions are
only as well timed as the audio is cut. Requests of up to 25 s that ran across pauses put
captions seconds early or late on a travel vlog (voice over music, several speakers).
Now every request is one stretch of speech between pauses (≤ 12 s), a caption never runs
across a pause of 0.5 s or more, and where the voice fills only part of a chunk an
installed offline model locates the words in it. Measured with
`scripts/bench/cue_window_check.py` (each caption's own time window is recognized again
on its own; the score is the share of the caption's letters heard in that window) and
`scripts/bench/srt_truth_check.py` (known sentence times):

| Video | Before | Now |
|---|---|---|
| Travel vlog, 4 min (music, chatter, 3+ speakers) | 0.68 | **0.83** |
| TV news, 2.5 min (continuous speech) | 0.96 | 0.96 |
| Read speech with known times, 5 min | — | 55 of 56 captions inside their sentence's true span (−0.3 / +0.5 s); the 56th is a scoring artifact |

Text accuracy is unchanged by the finer cut (13.4% WER on the 5-minute test, with digits
compared like-for-like). A 12-minute vlog takes 33 s. What stays hard: two voices at
once with no pause between them (the caption covers the whole stretch), and background
chatter that even a second listen transcribes differently.

Decisions that followed:
- The Bangla-trained models cut spelling errors by more than half, so Bangla speech is
  transcribed with them automatically when one is installed. **Bangla Max** is
  recommended on Apple-chip Macs (GPU makes it fast enough); **Bangla** on Windows and
  Intel Macs. The punctuation prompt is used only where it measurably helps (Bangla).
- Words the model was unsure of (confidence < 0.5) are listed under each caption as
  **Check** chips, so a careful read-through finds the likely misspellings quickly.
  No recognizer is 100% accurate; this is how the app gets you to 100% with review.
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

## Caption timing

Measured against the audio itself (`scripts/bench/timing_check.py`): each caption edge
next to a pause compared with where the voice really starts and stops.

| | Caption start vs voice | Caption end vs voice (incl. 0.15 s linger) |
|---|---|---|
| Before (Whisper word times) | 0.35–0.62 s off on Bangla | 0.52–0.74 s late |
| **Now** (DTW word alignment + VAD edges) | **0–0.1 s** | **0.05–0.3 s** |

Word times now come from whisper.cpp's DTW token alignment (`-dtw`, costs about 9% more
time on Apple-chip GPUs and 21% on CPU), and caption edges at pauses are placed on the
speech onsets/offsets found by the Silero VAD. In CapCut the SRT lands within one video
frame (CapCut snaps to frames).

## Auto-sync captions

Auto-sync moves existing captions to where their words are spoken, using only what runs
on the computer: the voice-activity detector finds the pauses, and an offline speech
model says which words lie between which pauses. The model's own word times are not
used: measured against exactly timed speech they were off by 1-2 s in about 15% of
captions, with or without attention alignment (DTW), greedy or beam search. Pauses are
exact, so a caption that begins after a pause lands on it; a caption that begins
mid-speech is placed by text length (good to about 0.3-0.45 s).

Test: `scripts/bench/make_phrase_audio.py` builds a 4.4-minute Bangla recording of 70
caption-sized phrases whose start and end are known exactly (gaps from 0.06 s to 1.5 s).
The exact captions are knocked out of sync and auto-synced (`scripts/bench/resync_check.py`):

| Captions were | Auto-sync found | Start error after (median / worst) | Within 0.3 s |
|---|---|---|---|
| all 2.5 s late | "offset", −2.5 s | 0.009 s / 0.29 s | 70 of 70 |
| drifting, 3% slow (up to 7.9 s late) | "drift", −0.03 s → −7.9 s | 0.036 s / 0.44 s | 91% |
| 1 in 6 moved 1.5-4 s at random | "mixed" | 0.000 s / see note | 93% |

Note on the third row: two captions had been moved past their neighbours (out of
order), which a matcher that keeps the order can't undo; both were marked for review.
Through the real interface (`scripts/dev/sync_check.py`): whole video and section-only
runs, preview, apply, "Check timing" marks, text and order untouched, Undo restores the
old times; at a section's edge a synced caption never crosses an untouched one.

Limits: needs an installed offline model for the captions' language; translated tracks
can't be synced (their words aren't the spoken words); captions over loud music or
overlapping voices are marked for review, not moved by guesswork; a 4.4-minute file
takes about 30-40 s on an M3 Pro.

## Long video

30.2-minute video of 126 real Bangla utterances with 0.5–3 s pauses, run through the real
app UI (Accurate model, before the Bangla model existed):
- **0 of 126 utterances without a caption**, 0 empty "needs review" captions
- 316 captions, all in order, 0 overlaps, last caption ends 0.6 s before speech ends
- CER 28.8% (one-pass Whisper scored 41–55% on comparable audio)
- ran at 1.32 × real time (with other benchmarks competing for the GPU)

## Automated tests

- Rust: 98 unit tests (`cargo test --lib`) — SRT timestamps/serialization/parsing,
  export validation and overlap fixing, project save/load/relink/atomicity, WAV slicing,
  VAD parsing and chunk planning, coverage gaps, Whisper JSON parsing (incl. invalid
  UTF-8 from real output), word confidence, segmentation (Bangla fixture), Bengali script
  repair, prompt-echo filtering, language voting, translation routes, resumable verified
  downloads (local HTTP server), job cancellation and scratch cleanup, balanced caption
  breaks (no lonely last word), spacing after sentence marks.
- Argos parity: CaptionNest's Rust translator gives the same English → Bangla output as
  the reference Python `argostranslate` (ctranslate2 4.9.1) on the fixture sentences.
- Frontend: 22 Vitest tests — cue editing (split/merge/add/delete/offset), undo history,
  timecode parsing.
- CI (GitHub Actions) builds every platform from pinned sources, runs both test suites,
  installs the package and runs the bundled pipeline on synthesized speech.

## Real-app checks (driven through the interface)

Performed on macOS Apple Silicon through the app's normal screens, using a test-only
automation bridge (`--features automation`, not in release builds) to click and read
the UI. 18 of 18 pass:

| Check | Result |
|---|---|
| English speech → English captions, video path with spaces and Bengali characters | pass — punctuated, sentence-split |
| Bangla speech → Bangla captions (Bangla model) | pass — 100% Bengali script, balanced captions |
| Bangla speech → English captions (Bangla auto-detected) | pass |
| Hindi → Hindi, Arabic → Arabic | pass — Arabic caption editor is right-to-left |
| English captions → Add language → Bangla (Argos, offline) | pass — every caption keeps its exact timing |
| Timing markers at 0.5 s / 30 s / 56 s | pass — 0.51 / 30.0 / 56.1 s |
| Audio starting 2 s into the file; variable frame rate video | pass — 2.01 s; VFR detected, timing correct |
| Choose 2nd audio track; make preview copy | pass |
| Silence-only video | pass — "No speech found" with next steps |
| Video without sound; damaged file | pass — explained, blocked |
| Cancel, then start a new job | pass — no leftover scratch files |
| Edit, split, merge, retime, undo/redo, shift timing | pass |
| Save project, reopen; move the video and relink | pass |
| Export SRT | pass — UTF-8 without BOM, CRLF, numbered |
| Model download: stop at 147 MB, resume, SHA-256 verify | pass |
| 30-minute video | pass (above) |
| No internet (all traffic routed to a dead proxy) | pass — English and Bangla→English captions made; a download fails within seconds: "Couldn't reach the download server…" |
| Network watch while generating and translating | pass — 0 connections leave the computer |
| Caption style: presets, font, colors, position; live preview | pass — screenshots in `build/shots/style-*.png` |
| Export video with burned-in captions (Bangla, Highlight style) | pass — H.264 + original AAC, same 41.16 s length; Bengali conjuncts drawn correctly; no caption 0.12 s before a caption starts |

## CapCut import

The Bangla SRT exported by CaptionNest was imported into **CapCut Desktop (macOS)** with
*Captions → Add captions → Import file* on the original video. Reading the saved CapCut
project: **8 of 8 captions became separate text clips, text identical** (Bengali intact),
times within 0.053 s of the SRT (CapCut snaps to the project's 30 fps frames).

## Packages

| Platform | Built | Install / run check |
|---|---|---|
| macOS Apple Silicon | locally and in CI — `.dmg` 30 MB, app 68 MB, ad-hoc signed | CI mounts the dmg and runs the bundled pipeline (`--self-test`) on synthesized speech |
| macOS Intel | in CI (native Intel runner) | same self-test |
| Windows 10/11 x64 | in CI (NSIS installer) | CI installs silently and runs the self-test |

## Pending / limitations

- **Not tested by hand on a clean Windows PC or a real Intel Mac.** CI builds, installs
  and runs the full caption pipeline on both, but nobody has clicked through the UI there.
- **Unsigned.** macOS shows the "can't verify the developer" step (Open Anyway, once);
  Windows SmartScreen shows "More info → Run anyway". Signing needs an Apple Developer
  account and a Windows code-signing certificate.
- **Clean-Mac test (fresh user, quarantined download, Gatekeeper flow)** not run: no VM
  was available on the test machine.
- **macOS 13/14** not run (minimum supported is 13.3; built against that target).
- **Online recognition** needs the user's own Sarvam API key and internet; Sarvam's
  synchronous API times whole phrases, so caption edges come from CaptionNest's VAD and
  splits inside a phrase are placed in proportion to text length.
- **Accuracy.** Bangla with the Bangla models is a strong first draft (≈ 1 word in 4 needs
  a fix on hard read speech, less on clear speech). Review the **Check** words before
  export. Background music, overlapping speakers and strong accents lower accuracy.
- **Translation quality** follows the Argos/OPUS-MT packs: readable, literal, sometimes
  wrong on names and idioms. English → Bangla is direct; other pairs go through English.
