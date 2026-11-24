#!/usr/bin/env python3
"""Developer tool: build a long test video from real FLEURS Bangla speech.

Writes build/testmedia/long bangla <N> min.mp4 plus a reference transcript and the
exact start time of every utterance (for timing checks).
usage: make_long_video.py [minutes]   (needs a full FFmpeg on PATH, e.g. Homebrew)
"""
import csv
import json
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
fleurs = ROOT / "build" / "fleurs"
out_dir = ROOT / "build" / "testmedia"
tmp = out_dir / ".long"
tmp.mkdir(parents=True, exist_ok=True)
minutes = float(sys.argv[1]) if len(sys.argv) > 1 else 30
random.seed(7)

rows, seen = [], set()
for r in csv.reader(open(fleurs / "bn_dev.tsv", encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE):
    if r[0] in seen or not (fleurs / "dev" / r[1]).exists():
        continue
    seen.add(r[0])
    rows.append(r)

def dur(p):
    return float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(p)],
                                capture_output=True, text=True).stdout)

listing, refs, timeline, t = [], [], [], 0.0
for i, r in enumerate(rows):
    if t >= minutes * 60:
        break
    gap = round(random.uniform(0.5, 3.0), 2)
    clip = tmp / f"u{i:04}.wav"
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(fleurs / "dev" / r[1]), "-ar", "16000", "-ac", "1",
                    "-af", f"apad=pad_dur={gap}", str(clip)], check=True)
    d = dur(clip)
    listing.append(f"file '{clip}'")
    refs.append(r[2])
    timeline.append({"start": round(t, 2), "speech_end": round(t + d - gap, 2), "text": r[2]})
    t += d

(tmp / "list.txt").write_text("\n".join(listing) + "\n")
name = f"long bangla {round(t / 60)} min"
subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(tmp / "list.txt"), str(tmp / "all.wav")], check=True)
subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"testsrc2=size=640x360:rate=15:duration={t}",
                "-i", str(tmp / "all.wav"), "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "32",
                "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "96k", "-shortest", str(out_dir / f"{name}.mp4")], check=True)
(out_dir / f"{name} - reference.txt").write_text("\n".join(refs) + "\n", encoding="utf-8")
(out_dir / f"{name} - timeline.json").write_text(json.dumps(timeline, ensure_ascii=False, indent=0), encoding="utf-8")
print(f"{name}: {len(refs)} utterances, {t / 60:.1f} min")
