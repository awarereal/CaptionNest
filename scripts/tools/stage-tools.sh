#!/usr/bin/env bash
# Copy the built tools for one target into src-tauri/resources so Tauri bundles them.
#
#   scripts/tools/stage-tools.sh aarch64-apple-darwin|x86_64-apple-darwin
#
# macOS binaries are ad-hoc signed so they run inside the (ad-hoc signed) app bundle.
set -euo pipefail

TRIPLE="${1:?usage: stage-tools.sh <target-triple>}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=versions.env
source "$ROOT/scripts/tools/versions.env"
SRC="$ROOT/build/tools/$TRIPLE"
RES="$ROOT/src-tauri/resources"

for f in ffmpeg ffprobe whisper-cli; do
  [[ -x "$SRC/$f" ]] || { echo "missing $SRC/$f - build the tools first" >&2; exit 1; }
done
"$ROOT/scripts/tools/fetch-sources.sh" >/dev/null

rm -rf "$RES/tools" "$RES/bundled-models"
mkdir -p "$RES/tools" "$RES/bundled-models" "$RES/licenses"
cp "$SRC/ffmpeg" "$SRC/ffprobe" "$SRC/whisper-cli" "$RES/tools/"
cp "$ROOT/build/src/$VAD_MODEL_NAME" "$RES/bundled-models/"
cp "$SRC/FFMPEG_CONFIGURE.txt" "$RES/licenses/FFMPEG_BUILD.txt"

if [[ "$(uname -s)" == "Darwin" ]]; then
  for f in "$RES"/tools/*; do
    codesign --force --sign - --timestamp=none "$f"
  done
  codesign --verify --strict "$RES"/tools/*
fi

echo "staged tools for $TRIPLE:"
ls -la "$RES/tools" "$RES/bundled-models"
