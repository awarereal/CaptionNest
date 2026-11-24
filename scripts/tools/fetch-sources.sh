#!/usr/bin/env bash
# Download the pinned third-party sources into build/src and verify their SHA-256 hashes.
# Safe to re-run: files that already match their hash are kept.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=versions.env
source "$ROOT/scripts/tools/versions.env"
SRC="$ROOT/build/src"
mkdir -p "$SRC"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

fetch() { # url file sha
  local url="$1" file="$SRC/$2" want="$3"
  if [[ -f "$file" && "$(sha256_of "$file")" == "$want" ]]; then
    echo "ok      $2"; return
  fi
  echo "fetch   $2"
  curl -fL --retry 3 -o "$file.part" "$url"
  local got; got="$(sha256_of "$file.part")"
  if [[ "$got" != "$want" ]]; then
    echo "SHA-256 mismatch for $2: expected $want, got $got" >&2
    rm -f "$file.part"; exit 1
  fi
  mv "$file.part" "$file"
}

fetch "$FFMPEG_URL" "ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256"
fetch "$WHISPER_URL" "whisper.cpp-$WHISPER_VERSION.tar.gz" "$WHISPER_SHA256"
fetch "$VAD_MODEL_URL" "$VAD_MODEL_NAME" "$VAD_MODEL_SHA256"
