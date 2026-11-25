#!/usr/bin/env python3
"""First Sarvam speech-to-text call: transcribes a short Bangla WAV with saaras:v4.

The key is read from the SARVAM_API_KEY environment variable or the project's .env
file; it is never printed.

usage: build/venv/bin/python scripts/dev/sarvam_first_call.py <audio.wav>
"""
import os
import sys
import time
from pathlib import Path

from sarvamai import SarvamAI


def api_key() -> str:
    key = os.environ.get("SARVAM_API_KEY")
    env = Path(__file__).resolve().parents[2] / ".env"
    if not key and env.exists():
        for line in env.read_text().splitlines():
            if line.startswith("SARVAM_API_KEY="):
                key = line.split("=", 1)[1].strip()
    if not key:
        sys.exit("SARVAM_API_KEY is not set (environment or .env).")
    return key


client = SarvamAI(api_subscription_key=api_key())
t0 = time.time()
with open(sys.argv[1], "rb") as f:
    r = client.speech_to_text.transcribe(file=f, model="saaras:v4", mode="transcribe", language_code="bn-IN", with_timestamps=True)
print(f"took {time.time() - t0:.2f}s  language={r.language_code}  request_id={r.request_id}")
print("transcript:", r.transcript)
if r.timestamps:
    for w, a, b in zip(r.timestamps.words, r.timestamps.start_time_seconds, r.timestamps.end_time_seconds):
        print(f"  {a:6.2f}-{b:6.2f}  {w}")
