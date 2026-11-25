#!/usr/bin/env python3
"""Builds a Bangla test recording whose caption times are known exactly.

Real Bangla sentences (the FLEURS reference text) are cut into caption-sized phrases;
each phrase is spoken on its own by the macOS Bangla voice and placed at a known time,
separated by gaps of mixed length: barely a breath (continuous speech), a short pause,
a real pause. Writes the WAV and an SRT with the exact start and end of every phrase.

usage: make_phrase_audio.py <out_basename> [phrases]
"""
import json
import os
import random
import re
import subprocess
import sys
import tempfile
import wave

import numpy as np

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
out_base = sys.argv[1]
want = int(sys.argv[2]) if len(sys.argv) > 2 else 70
timeline = json.load(open(os.path.join(ROOT, "build", "testmedia", "long bangla 30 min - timeline.json"), encoding="utf-8"))

phrases = []
for u in timeline:
    # Sentences with Latin letters or digits are read unpredictably by the voice.
    if re.search(r"[A-Za-z0-9০-৯]", u["text"]):
        continue
    words = u["text"].replace(";", ",").split()
    cur = []
    for w in words:
        cur.append(w)
        if (w[-1] in ",।?!" and len(cur) >= 3) or len(cur) >= 7:
            phrases.append(" ".join(cur))
            cur = []
    if len(cur) >= 3:
        phrases.append(" ".join(cur))
    elif cur and phrases:
        phrases[-1] += " " + " ".join(cur)
    if len(phrases) >= want:
        break
phrases = phrases[:want]

random.seed(11)
rate = 16000
work = tempfile.mkdtemp(prefix="phrases-")
audio = [np.zeros(int(rate * 1.2), dtype=np.int16)]
at = 1.2
cues = []
for i, text in enumerate(phrases):
    aiff = os.path.join(work, f"p{i}.aiff")
    subprocess.run(["say", "-v", "Piya", "-o", aiff, text], check=True)
    wav = os.path.join(work, f"p{i}.wav")
    subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", aiff, wav], check=True)
    with wave.open(wav) as w:
        x = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)
    # Trim the voice's own leading and trailing silence, so the times are the speech.
    loud = np.flatnonzero(np.abs(x.astype(np.int32)) > 250)
    x = x[max(0, loud[0] - 160): loud[-1] + 160]
    cues.append((at, at + len(x) / rate, text))
    audio.append(x)
    at += len(x) / rate
    gap = random.choice([random.uniform(0.06, 0.2)] * 4 + [random.uniform(0.3, 0.55)] * 3 + [random.uniform(0.7, 1.5)] * 3)
    audio.append(np.zeros(int(rate * gap), dtype=np.int16))
    at += int(rate * gap) / rate
audio.append(np.zeros(rate, dtype=np.int16))
x = np.concatenate(audio).astype(np.float32)
x += np.random.default_rng(3).normal(0, 12, len(x)).astype(np.float32)  # faint room noise
with wave.open(out_base + ".wav", "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(rate)
    w.writeframes(np.clip(x, -32768, 32767).astype(np.int16).tobytes())


def stamp(sec):
    ms = int(round(sec * 1000))
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"


with open(out_base + ".srt", "w", encoding="utf-8", newline="") as f:
    for i, (a, b, text) in enumerate(cues, 1):
        f.write(f"{i}\r\n{stamp(a)} --> {stamp(b)}\r\n{text}\r\n\r\n")
print(f"{len(cues)} phrases, {at:.1f} s -> {out_base}.wav / .srt")
