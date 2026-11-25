#!/usr/bin/env python3
"""Which Sarvam models return word-level timestamps? Sends one WAV to each model with
with_timestamps=true and prints how many timed entries come back.

usage: build/venv/bin/python -I scripts/dev/sarvam_timestamps.py <audio.wav>
The key comes from SARVAM_API_KEY or .env; it is never printed.
"""
import json
import os
import sys
from pathlib import Path

import httpx as requests

key = os.environ.get("SARVAM_API_KEY") or next(
    line.split("=", 1)[1].strip()
    for line in (Path(__file__).resolve().parents[2] / ".env").read_text().splitlines()
    if line.startswith("SARVAM_API_KEY=")
)
for model in ["saarika:v2.5", "saaras:v3", "saaras:v4"]:
    with open(sys.argv[1], "rb") as f:
        r = requests.post(
            "https://api.sarvam.ai/speech-to-text",
            headers={"api-subscription-key": key},
            files={"file": ("a.wav", f, "audio/wav")},
            data={"model": model, "language_code": "bn-IN", "with_timestamps": "true"},
            timeout=60,
        )
    print(model, r.status_code)
    try:
        d = r.json()
    except ValueError:
        print("  ", r.text[:300])
        continue
    if r.status_code != 200:
        print("  ", json.dumps(d, ensure_ascii=False)[:300])
        continue
    ts = d.get("timestamps") or {}
    words = ts.get("words") or []
    print("   entries:", len(words))
    for w, a, b in list(zip(words, ts.get("start_time_seconds") or [], ts.get("end_time_seconds") or []))[:8]:
        print(f"   {a:6.2f}-{b:6.2f} {w[:60]!r}")
