#!/bin/bash
# Developer comparison: auto-sync accuracy with different listening settings, on the
# exactly timed phrase recording (offset case only).
# usage: scripts/bench/resync_modes.sh <audio> <exact.srt> <language>
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
run() { # label, env assignments...
  local label="$1"
  shift
  echo "== $label"
  env CASES=offset "$@" python3 "$ROOT/scripts/bench/resync_check.py" "$AUDIO" "$SRT" "$LANG_CODE" "mode-$label" 2>&1 | tail -1 |
    python3 -c "
import sys, json
line = sys.stdin.read().strip()
d = json.loads(line[line.index('{'):])
print('  seconds', d['seconds'], '| found', d['found'], '| matched', d['matched'], '/', d['captions'], '| after:', d['start_error_after_ms'], '| within 0.3 s:', d['within_300ms'])"
}
AUDIO="$1"
SRT="$2"
LANG_CODE="$3"
run greedy-dtw CAPTIONNEST_SYNC_MODE=greedy-dtw
run beam-dtw CAPTIONNEST_SYNC_MODE=beam-dtw
run beam-plain CAPTIONNEST_SYNC_MODE=beam
run greedy-plain CAPTIONNEST_SYNC_MODE=greedy
run beam-dtw-full-window CAPTIONNEST_SYNC_MODE=beam-dtw CAPTIONNEST_NO_AC=1
