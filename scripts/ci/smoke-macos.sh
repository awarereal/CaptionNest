#!/usr/bin/env bash
# Smoke test a built macOS .dmg the way a recipient would use it:
# mount the disk image, copy the app out, and run its bundled pipeline on known speech.
#   scripts/ci/smoke-macos.sh <path/to/CaptionNest.dmg> <ggml-model.bin>
set -euo pipefail

DMG="$1"
MODEL="$2"
WORK="$(mktemp -d)"
MNT="$WORK/mnt"
mkdir -p "$MNT" "$WORK/Applications"

hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$DMG" >/dev/null
trap 'hdiutil detach "$MNT" -quiet || true' EXIT
cp -R "$MNT"/*.app "$WORK/Applications/"
APP="$(ls -d "$WORK"/Applications/*.app)"
BIN="$(ls "$APP"/Contents/MacOS/* | head -n 1)"

echo "Bundle signature:"
codesign --verify --deep --strict --verbose=2 "$APP"
echo "Bundled tools link only to system libraries:"
for t in "$APP"/Contents/Resources/tools/*; do
  otool -L "$t" | tail -n +2 | awk '{print $1}' | grep -vE '^/(usr/lib|System/Library)/' && { echo "non-system dependency in $t" >&2; exit 1; } || true
done

say -v Samantha "Hello from the CaptionNest smoke test. The quick brown fox jumps over the lazy dog." -o "$WORK/speech.aiff"
# A clean PATH proves the app uses its own bundled tools, not developer tools.
env -i HOME="$HOME" PATH=/usr/bin:/bin CAPTIONNEST_HOME="$WORK/home" \
  "$BIN" --self-test "$WORK/speech.aiff" --model "$MODEL" --out "$WORK/out.srt" --language en || true

cat "$WORK/out.json"
python3 - "$WORK/out.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
assert r["ok"], r
text = r["text"].lower()
assert "fox" in text and "dog" in text, text
print("smoke test passed:", r["cues"], "captions in", round(r["seconds"], 1), "s, GPU:", r["usedGpu"])
PY
head -n 8 "$WORK/out.srt"
