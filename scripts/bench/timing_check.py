#!/usr/bin/env python3
"""Caption timing accuracy against the audio itself.

Runs the headless pipeline (--self-test) on test videos, then compares every caption
edge that sits next to a pause with where speech energy actually starts or stops.
Positive start error = caption appears late; positive end error = caption lingers.

usage: timing_check.py <captionnest binary> <models dir> [label]
(REUSE=1 re-scores SRT files from an earlier run instead of generating them.)
"""
import json
import os
import subprocess
import sys

import numpy as np

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN, MODELS = sys.argv[1], sys.argv[2]
LABEL = sys.argv[3] if len(sys.argv) > 3 else "run"
FFMPEG = os.path.join(os.path.dirname(BIN), "tools", "ffmpeg")
OUT = os.path.join(ROOT, "build", "bench", "timing")
os.makedirs(OUT, exist_ok=True)

CASES = [
    ("english", "টেস্ট ভিডিও - English sample.mp4", "ggml-large-v3-q5_0.bin", "en"),
    ("bangla", "বাংলা নমুনা (Bangla sample).mp4", "ggml-large-v3-turbo-bn-q5_0.bin", "bn"),
    ("timing", "timing 0.5s 30s 56s.mp4", "ggml-large-v3-q5_0.bin", "en"),
]


def audio(path):
    raw = subprocess.run([FFMPEG, "-v", "error", "-i", path, "-ac", "1", "-ar", "16000", "-f", "s16le", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768


def speech_regions(x, hop=160):
    """10 ms frames; speech = energy 18 dB above the noise floor; merge gaps < 250 ms."""
    n = len(x) // hop
    e = 10 * np.log10(np.mean(x[: n * hop].reshape(n, hop) ** 2, axis=1) + 1e-10)
    floor = np.percentile(e, 10)
    # Room noise in recordings sits far below the voice: require both a margin over the
    # floor and closeness to the loud (voiced) frames.
    on = e > max(floor + 18, np.percentile(e, 95) - 24)
    regions, start, gap, last = [], None, 0, 0
    for i, v in enumerate(on):
        if v:
            if start is None:
                start = i
            gap, last = 0, i
        elif start is not None:
            gap += 1
            if gap > 25:
                regions.append((start * 10, (last + 1) * 10))
                start = None
    if start is not None:
        regions.append((start * 10, (last + 1) * 10))
    return [(a, b) for a, b in regions if b - a >= 120]


def srt(path):
    def t(s):
        h, m, rest = s.split(":")
        sec, ms = rest.split(",")
        return ((int(h) * 60 + int(m)) * 60 + int(sec)) * 1000 + int(ms)
    cues = []
    for block in open(path, encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
        lines = block.split("\n")
        a, b = lines[1].split(" --> ")
        cues.append((t(a), t(b), " ".join(lines[2:])))
    return cues


report = {}
for name, video, model, lang in CASES:
    vpath = os.path.join(ROOT, "build", "testmedia", video)
    out = os.path.join(OUT, f"{LABEL}-{name}.srt")
    if not (os.environ.get("REUSE") and os.path.exists(out)):
        subprocess.run([BIN, "--self-test", vpath, "--model", os.path.join(MODELS, model), "--out", out, "--language", lang],
                       check=True, capture_output=True)
    cues = srt(out)
    regions = speech_regions(audio(vpath))
    starts, ends = [], []
    for a, b, _ in cues:
        on = [r[0] for r in regions if abs(r[0] - a) <= 700]
        off = [r[1] for r in regions if abs(r[1] - b) <= 900]
        if on:
            starts.append(a - min(on, key=lambda o: abs(o - a)))
        if off:
            ends.append(b - min(off, key=lambda o: abs(o - b)))
    report[name] = {
        "cues": len(cues), "speech_regions": len(regions),
        "start_errors_ms": starts, "end_errors_ms": ends,
        "start_mean_abs": round(float(np.mean(np.abs(starts))), 1) if starts else None,
        "end_mean_abs": round(float(np.mean(np.abs(ends))), 1) if ends else None,
    }
    print(name, json.dumps(report[name]))
json.dump(report, open(os.path.join(OUT, f"{LABEL}.json"), "w"), indent=1)
