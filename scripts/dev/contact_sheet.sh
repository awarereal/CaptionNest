#!/bin/bash
# Puts up to four screenshots side by side (2x2) in one image, for quick review.
# usage: scripts/dev/contact_sheet.sh out.png a.png b.png [c.png d.png]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FF="$ROOT/src-tauri/target/release/tools/ffmpeg"
out="$1"
shift
args=()
n=0
for f in "$@"; do
  args+=(-i "$f")
  n=$((n + 1))
done
# Every tile 1100 px wide, letterboxed to the same height so they line up.
prep=""
for i in $(seq 0 $((n - 1))); do
  prep+="[$i:v]scale=1100:-2,pad=1100:760:0:0:color=0x333333[t$i];"
done
case $n in
  1) layout="[t0]copy" ;;
  2) layout="[t0][t1]hstack=inputs=2" ;;
  3) layout="[t0][t1][t2]xstack=inputs=3:layout=0_0|w0_0|0_h0:fill=0x333333" ;;
  *) layout="[t0][t1][t2][t3]xstack=inputs=4:layout=0_0|w0_0|0_h0|w0_h0" ;;
esac
"$FF" -nostdin -v error -y "${args[@]}" -filter_complex "${prep}${layout}" -frames:v 1 "$out"
