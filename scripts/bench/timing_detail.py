#!/usr/bin/env python3
"""Print speech regions next to caption times for one test video (timing debugging).

usage: timing_detail.py <ffmpeg> <video> <srt>
"""
import subprocess
import sys

import numpy as np

ffmpeg, video, srt_path = sys.argv[1:4]
raw = subprocess.run([ffmpeg, "-v", "error", "-i", video, "-ac", "1", "-ar", "16000", "-f", "s16le", "-"],
                     capture_output=True, check=True).stdout
x = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768
hop = 160
n = len(x) // hop
e = 10 * np.log10(np.mean(x[: n * hop].reshape(n, hop) ** 2, axis=1) + 1e-10)
floor = np.percentile(e, 10)
on = e > max(floor + 18, e.max() - 45)
# Print speech runs with gaps >= 80 ms as separate regions.
runs, start, gap, last = [], None, 0, 0
for i, v in enumerate(on):
    if v:
        if start is None:
            start = i
        gap, last = 0, i
    elif start is not None:
        gap += 1
        if gap > 8:
            runs.append((start * 10, (last + 1) * 10))
            start = None
if start is not None:
    runs.append((start * 10, (last + 1) * 10))
print("speech:", [(a, b) for a, b in runs if b - a >= 60])
for block in open(srt_path, encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
    lines = block.split("\n")
    print("cue:", lines[1], " ".join(lines[2:])[:50])
