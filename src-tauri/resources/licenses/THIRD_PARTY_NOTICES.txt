CaptionNest — third-party notices
=================================

CaptionNest is distributed under the MIT License (see LICENSE). It bundles or
downloads the components below. Full license texts are in the "licenses"
folder inside the app and in the source repository.


Bundled programs
----------------

FFmpeg 9.0.2 (ffmpeg, ffprobe)
  License: GNU Lesser General Public License, version 2.1 or later (LGPL-2.1+)
  This software uses code of FFmpeg (https://ffmpeg.org) licensed under the
  LGPLv2.1 and its source can be downloaded from the CaptionNest release page
  (file "ffmpeg-9.0.2.tar.xz", unmodified, identical to
  https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz,
  SHA-256 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e).
  FFmpeg is built without GPL or non-free parts and without external codec
  libraries. The exact configure options are in licenses/FFMPEG_BUILD.txt and
  the build scripts are in the CaptionNest source repository
  (scripts/tools/). FFmpeg runs as a separate program; CaptionNest does not
  link to it. FFmpeg is a trademark of Fabrice Bellard, originator of the
  FFmpeg project. CaptionNest does not own FFmpeg.

whisper.cpp v1.9.6 and ggml (whisper-cli)
  License: MIT — Copyright (c) 2023-2026 The ggml authors
  https://github.com/ggml-org/whisper.cpp

Silero VAD v6.2.0 (ggml conversion, bundled voice activity model)
  License: MIT — Copyright (c) 2020-present Silero Team
  https://github.com/snakers4/silero-vad
  ggml conversion: https://huggingface.co/ggml-org/whisper-vad


Linked libraries
----------------

CTranslate2 — MIT — Copyright (c) 2019 SYSTRAN, 2020-present OpenNMT
ct2rs / ct2rs-sys — MIT — Copyright (c) 2023-2026 Junpei Kawamoto
SentencePiece — Apache License 2.0 — Copyright (c) Google LLC
sentencepiece (Rust bindings) — MIT or Apache-2.0 — Daniël de Kok
ruy — Apache License 2.0 — Copyright Google LLC
Tauri and its plugins — MIT or Apache-2.0 — The Tauri Programme within The
  Commons Conservancy
Other Rust crates — see licenses/DEPENDENCIES.txt (MIT, Apache-2.0, BSD,
  ISC, Zlib, Unicode-3.0 and similar permissive licenses)


Interface
---------

React, React DOM — MIT — Copyright (c) Meta Platforms, Inc. and affiliates
Radix UI Primitives — MIT — Copyright (c) 2022 WorkOS
cmdk — MIT — Copyright (c) 2022 Paco Coursey
Sonner — MIT — Copyright (c) 2023 Emil Kowalski
Motion — MIT — Copyright (c) 2018 Framer B.V.
TanStack Virtual — MIT — Copyright (c) 2021-present Tanner Linsley
Zustand — MIT — Copyright (c) 2019 Paul Henschel
Phosphor Icons — MIT — Copyright (c) 2020 Phosphor Icons
Anek Latin, Anek Bangla, Anek Devanagari — SIL Open Font License 1.1 —
  Copyright 2021 The Anek Project Authors (Ek Type)
Atkinson Hyperlegible Next and Atkinson Hyperlegible Mono — SIL Open Font
  License 1.1 — Copyright 2020-2024 Braille Institute of America, Inc.
Readex Pro — SIL Open Font License 1.1 — Copyright 2021 The Readex Pro
  Project Authors
Caption fonts (offered in Caption style and drawn into exported videos), all
SIL Open Font License 1.1:
  Hind Siliguri — Copyright (c) 2015 Indian Type Foundry
  Noto Sans — Copyright 2022 The Noto Project Authors
  Noto Sans Bengali — Copyright 2025 The Noto Project Authors
  Baloo Da 2 — Copyright 2019 The Baloo 2 Project Authors (Ek Type)
  Atma — Copyright 2015 The Atma Project Authors (Black Foundry)
  Tiro Bangla — Copyright 2020 The Indigo Project Authors (Tiro Typeworks)
  The OFL allows these fonts to be used in videos you make, including
  commercially.
Fonts are bundled through the Fontsource packages (MIT).


Online services (optional, used only with the user's own API keys)
-------------------------------------------------------------------
Sarvam AI speech-to-text (https://www.sarvam.ai). Nothing from Sarvam is bundled;
when the user turns on online recognition, the speech audio is sent to Sarvam's API
under Sarvam's terms of service.

Deepgram speech-to-text (https://deepgram.com). Nothing from Deepgram is bundled;
when the user dictates with voice typing, the microphone recording is sent to
Deepgram's API under Deepgram's terms of service.


Downloaded on request (not included in the installer)
------------------------------------------------------

These files are downloaded only when you choose to, directly from their
publishers, and are checked against a pinned SHA-256 fingerprint.

OpenAI Whisper models (ggml format)
  License: MIT — Copyright (c) 2022 OpenAI
  Source: https://huggingface.co/ggerganov/whisper.cpp

Bangla speech models (Mozilla AI fine-tunes of Whisper)
  "Bangla" = mozilla-ai/whisper-large-v3-turbo-bn, "Bangla Max" =
  mozilla-ai/whisper-large-v3-bn (https://huggingface.co/mozilla-ai).
  License: Apache License 2.0. CaptionNest converted the published weights to
  whisper.cpp format and quantized them to q5_0 (no other changes) and hosts
  them at https://github.com/awarereal/CaptionNest/releases/tag/models-bangla-v1
  together with the license and a NOTICE describing the conversion.

Argos Translate language packages
  Argos Translate is released under the MIT License and CC0
  (https://github.com/argosopentech/argos-translate).
  Packages are downloaded from https://www.argos-net.com. Each package
  includes a README describing its training data and license. Many packages,
  including Bangla (bn<->en), are derived from OPUS-MT models by Jörg
  Tiedemann and Santhosh Thottingal, licensed CC BY 4.0
  (https://github.com/Helsinki-NLP/Opus-MT). Some packages (for example the
  Hindi and Arabic ones) credit OPUS training data and Stanza (Apache-2.0)
  but do not state a license for the model itself; check the package README
  if you plan to redistribute translations commercially.
  CaptionNest runs these packages with its own Rust code; it does not
  include Argos Translate's Python code.
