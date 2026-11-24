#!/usr/bin/env python3
"""Developer prototype: transcribe VAD-based speech chunks in one whisper-cli run.

Compares CER with the chunked approach against the reference text.
usage: chunk_test.py <wav16k> <reference.txt> <model> <lang>
"""
import json
import re
import subprocess
import sys
import time
import wave

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

T = "src-tauri/resources/tools"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
PROMPTS = {"bn": "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?", "en": "Hello, and welcome. This is how we speak."}
wav_path, ref_file, model, lang = sys.argv[1:5]
MAX = int(sys.argv[5]) * 1000 if len(sys.argv) > 5 else 28_000

out = subprocess.run([f"{T}/whisper-vad", "-f", wav_path, "-vm", VAD, "-np", "-vsd", "300", "-vp", "120", "-vmsd", str(max(5, MAX // 1000 - 2))],
                     capture_output=True, text=True).stdout
segs = [(round(float(a) * 10), round(float(b) * 10)) for a, b in re.findall(r"start = ([\d.]+), end = ([\d.]+)", out)]
chunks = []
for s, e in segs:
    if chunks and s - chunks[-1][1] < 1500 and e - chunks[-1][0] <= MAX:
        chunks[-1] = (chunks[-1][0], e)
    else:
        chunks.append((s, e))
with wave.open(wav_path) as w:
    rate, frames = w.getframerate(), w.readframes(w.getnframes())
files = []
for i, (s, e) in enumerate(chunks):
    a, b = max(0, s - 250), e + 250
    p = f"build/samples/chunk{i:03}.wav"
    with wave.open(p, "wb") as o:
        o.setnchannels(1); o.setsampwidth(2); o.setframerate(rate)
        o.writeframes(frames[int(a * rate / 1000) * 2:int(b * rate / 1000) * 2])
    files.append((p, a))
args = [f"{T}/whisper-cli", "-m", f"build/models/ggml-{model}.bin", "-l", lang, "-oj", "-ml", "1", "-sow", "-sns", "--prompt", PROMPTS[lang]]
for p, _ in files:
    args += ["-f", p]
t0 = time.time()
subprocess.run(args, capture_output=True)
el = time.time() - t0
text = ""
for p, off in files:
    d = json.loads(open(p + ".json", "rb").read().decode("utf-8", errors="replace"))
    text += "".join(s["text"] for s in d["transcription"])
if lang == "bn":
    text = "".join(chr(ord(c) + 0x80) if 0x0900 <= ord(c) <= 0x097F and ord(c) not in (0x964, 0x965) else c for c in text)
ref = norm(open(ref_file, encoding="utf-8").read())
h = norm(text)
cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
print(f"{model} chunks={len(chunks)} CER {cer*100:.1f}% time {el:.1f}s | {text.strip()[:400]}")
