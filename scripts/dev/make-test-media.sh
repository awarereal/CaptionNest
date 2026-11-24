#!/usr/bin/env bash
# Developer tool (macOS): creates test videos with known speech under build/testmedia.
# Uses the system `say` voices and a full-featured FFmpeg from PATH (e.g. Homebrew) to
# encode H.264 test videos. Real Bangla speech comes from FLEURS (build/fleurs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/build/testmedia"
TMP="$OUT/.tmp"
mkdir -p "$OUT" "$TMP"
FF="${FFMPEG:-ffmpeg}"
ff() { "$FF" -hide_banner -loglevel error -y "$@"; }

say_wav() { # voice text out.wav
  say -v "$1" "$2" -o "$TMP/say.aiff"
  ff -i "$TMP/say.aiff" -ar 48000 -ac 1 "$3"
}

video_with_audio() { # audio.wav seconds out.mp4 [label]
  ff -f lavfi -i "testsrc2=size=640x360:rate=30:duration=$2" -i "$1" \
     -map 0:v -map 1:a -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac -b:a 128k -shortest "$3"
}

# 1. English speech, file name with spaces and Bengali characters
say_wav Samantha "Welcome to CaptionNest. This short video checks English captions. The weather is lovely today, so we are going for a walk by the river." "$TMP/en.wav"
video_with_audio "$TMP/en.wav" 12 "$OUT/টেস্ট ভিডিও - English sample.mp4"

# 2. Arabic and Hindi speech (synthetic voices)
say_wav Majed "مرحبا بكم. هذا اختبار قصير للترجمة باللغة العربية. الطقس جميل اليوم." "$TMP/ar.wav"
video_with_audio "$TMP/ar.wav" 9 "$OUT/arabic sample.mp4"
say_wav Lekha "नमस्ते। यह हिंदी कैप्शन का एक छोटा परीक्षण है। आज मौसम बहुत अच्छा है।" "$TMP/hi.wav"
video_with_audio "$TMP/hi.wav" 9 "$OUT/hindi sample.mp4"

# 3. Real Bangla speech (FLEURS dev): three utterances with 2 s gaps, plus their texts
python3 - "$ROOT/build/fleurs" "$TMP" <<'PY'
import csv, sys, pathlib
root, tmp = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
rows, seen = [], set()
for r in csv.reader(open(root / "bn_dev.tsv", encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE):
    if r[0] in seen or not (root / "dev" / r[1]).exists():
        continue
    seen.add(r[0]); rows.append(r)
    if len(rows) == 3:
        break
(tmp / "bn_list.txt").write_text("\n".join(str(root / "dev" / r[1]) for r in rows) + "\n")
(tmp.parent / "bangla sample - reference.txt").write_text("\n".join(r[2] for r in rows) + "\n", encoding="utf-8")
PY
i=0; parts=()
while IFS= read -r f; do
  ff -i "$f" -ar 48000 -ac 1 "$TMP/bn$i.wav"
  ff -f lavfi -i anullsrc=r=48000:cl=mono -t 2 "$TMP/gap$i.wav"
  parts+=("$TMP/bn$i.wav" "$TMP/gap$i.wav"); i=$((i+1))
done < "$TMP/bn_list.txt"
args=(); for p in "${parts[@]}"; do args+=(-i "$p"); done
ff "${args[@]}" -filter_complex "concat=n=${#parts[@]}:v=0:a=1" "$TMP/bn.wav"
dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$TMP/bn.wav")
video_with_audio "$TMP/bn.wav" "$dur" "$OUT/বাংলা নমুনা (Bangla sample).mp4"

# 4. Timing at the beginning, middle and end of a 60 s video
say_wav Samantha "Beginning marker." "$TMP/t1.wav"
say_wav Samantha "Middle marker." "$TMP/t2.wav"
say_wav Samantha "Final marker." "$TMP/t3.wav"
ff -f lavfi -i anullsrc=r=48000:cl=mono -t 60 -i "$TMP/t1.wav" -i "$TMP/t2.wav" -i "$TMP/t3.wav" -filter_complex \
  "[1]adelay=500|500[a];[2]adelay=30000|30000[b];[3]adelay=56000|56000[c];[0][a][b][c]amix=inputs=4:normalize=0,atrim=0:60" "$TMP/timing.wav"
video_with_audio "$TMP/timing.wav" 60 "$OUT/timing 0.5s 30s 56s.mp4"

# 5. Audio track starting 2 s after the video (start-offset handling)
ff -f lavfi -i "testsrc2=size=640x360:rate=30:duration=14" -itsoffset 2 -i "$TMP/en.wav" \
   -map 0:v -map 1:a -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac "$OUT/delayed audio 2s.mp4"

# 6. Variable frame rate video with speech
ff -f lavfi -i "testsrc2=size=640x360:rate=30:duration=12" -i "$TMP/en.wav" \
   -filter:v "setpts='if(lt(T,6),PTS,PTS*1.0)',select='not(mod(n\,3))+lt(t\,6)'" -fps_mode vfr \
   -map 0:v -map 1:a -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac -shortest "$OUT/variable frame rate.mp4"

# 7. Two audio tracks in MKV (English, Bangla) - track choice and preview conversion
ff -f lavfi -i "testsrc2=size=640x360:rate=25:duration=12" -i "$TMP/en.wav" -i "$TMP/bn0.wav" \
   -map 0:v -map 1:a -map 2:a -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a libopus \
   -metadata:s:a:0 language=eng -metadata:s:a:0 title="English" -metadata:s:a:1 language=ben -metadata:s:a:1 title="বাংলা" \
   -shortest "$OUT/two tracks.mkv"

# 8. Silence only, and no audio at all
ff -f lavfi -i "testsrc2=size=640x360:rate=30:duration=10" -f lavfi -i anullsrc=r=48000:cl=stereo \
   -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac -shortest "$OUT/silence only.mp4"
ff -f lavfi -i "testsrc2=size=640x360:rate=30:duration=5" -c:v libx264 -pix_fmt yuv420p -preset veryfast -an "$OUT/no audio.mp4"

# 9. A damaged file
head -c 200000 "$OUT/টেস্ট ভিডিও - English sample.mp4" | tail -c 150000 > "$OUT/damaged.mp4"

rm -rf "$TMP"
ls -la "$OUT"
