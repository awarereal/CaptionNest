#!/usr/bin/env bash
# Developer tool: download Mozilla AI's Bengali Whisper fine-tunes (Apache-2.0) for
# conversion to whisper.cpp format. Files go to build/hf/<repo>.
set -euo pipefail
cd "$(dirname "$0")/../.."
for repo in whisper-large-v3-turbo-bn whisper-large-v3-bn; do
  dir="build/hf/$repo"
  mkdir -p "$dir"
  files=$(curl -fsSL "https://huggingface.co/api/models/mozilla-ai/$repo/tree/main" |
    python3 -c "import json,sys; print(' '.join(f['path'] for f in json.load(sys.stdin) if f['type']=='file' and f['path'] not in ('.gitattributes','training_args.bin')))")
  for f in $files; do
    curl -fsSL -C - --retry 5 -o "$dir/$f" "https://huggingface.co/mozilla-ai/$repo/resolve/main/$f" &
  done
  wait
  echo "$repo done: $(du -sh "$dir" | cut -f1)"
done
