#!/bin/bash
# Developer helper: run the headless pipeline with online recognition on one video.
# usage: scripts/bench/run_online.sh <video> <out.srt> [language] [seconds-to-keep]
# Reads SARVAM_API_KEY from .env (never printed). With seconds-to-keep, only the first
# part of the video is used (a stream copy in the scratch folder).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VIDEO="$1"
OUT="$2"
LANG_CODE="${3:-auto}"
KEEP="${4:-}"
set -a
. "$ROOT/.env"
set +a
if [ -n "$KEEP" ]; then
  CLIP="$(dirname "$OUT")/clip-$KEEP.mkv"
  "$ROOT/src-tauri/target/release/tools/ffmpeg" -nostdin -v error -y -i "$VIDEO" -t "$KEEP" -c copy "$CLIP"
  VIDEO="$CLIP"
fi
CAPTIONNEST_HOME="$ROOT/build/devhome" "$ROOT/src-tauri/target/release/captionnest" --self-test "$VIDEO" \
  --model "$ROOT/build/devhome/data/models/whisper/ggml-large-v3-turbo-bn-q5_0.bin" \
  --out "$OUT" --language "$LANG_CODE" --online > "${OUT%.srt}.log" 2>&1 || true
python3 -c "
import json,sys
d=json.load(open(sys.argv[1]))
print({k:d.get(k) for k in ('ok','seconds','modelUsed','warnings','language','cues','error')})" "${OUT%.srt}.json"
