#!/usr/bin/env python3
"""Developer check: speed vs accuracy of decoding options on chunked recognition.

Uses the first N minutes of the long Bangla test video (real FLEURS speech), recognizes
it the way the app does (VAD chunks of <=12 s, one whisper-cli run), and reports CER and
real-time factor for each option set.

usage: speed_test.py <model> [minutes] [lang]
"""
import json
import math
import re
import subprocess
import sys
import time
import wave
from pathlib import Path

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

T = "src-tauri/resources/tools"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
PROMPT = {"bn": "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?"}
model = sys.argv[1]
minutes = float(sys.argv[2]) if len(sys.argv) > 2 else 5
lang = sys.argv[3] if len(sys.argv) > 3 else "bn"
base = "build/testmedia/long bangla 30 min"
limit = minutes * 60
work = Path("build/samples/speed")
work.mkdir(parents=True, exist_ok=True)

subprocess.run([f"{T}/ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", base + ".mp4", "-t", str(limit), "-ac", "1", "-ar", "16000",
                "-c:a", "pcm_s16le", str(work / "in.wav")], check=True)
timeline = json.load(open(base + " - timeline.json", encoding="utf-8"))
ref = norm(" ".join(u["text"] for u in timeline if u["speech_end"] <= limit))

out = subprocess.run([f"{T}/whisper-vad", "-f", str(work / "in.wav"), "-vm", VAD, "-np", "-vsd", "300", "-vp", "120", "-vmsd", "10"],
                     capture_output=True, text=True).stdout
segs = [(round(float(a) * 10), round(float(b) * 10)) for a, b in re.findall(r"start = ([\d.]+), end = ([\d.]+)", out)]
chunks = []
for s, e in segs:
    if chunks and s - chunks[-1][1] < 1500 and e - chunks[-1][0] <= 12_000:
        chunks[-1] = (chunks[-1][0], e)
    else:
        chunks.append((s, e))
with wave.open(str(work / "in.wav")) as w:
    rate, frames = w.getframerate(), w.readframes(w.getnframes())
files = []
for i, (s, e) in enumerate(chunks):
    a, b = max(0, s - 250), e + 250
    p = work / f"c{i:04}.wav"
    with wave.open(str(p), "wb") as o:
        o.setnchannels(1); o.setsampwidth(2); o.setframerate(rate)
        o.writeframes(frames[int(a * rate / 1000) * 2:int(b * rate / 1000) * 2])
    files.append(p)
longest = max(b - a for a, b in chunks) + 500
ac = min(1500, int(math.ceil(longest / 30_000 * 1500 / 64) * 64) + 64)
speech_s = sum(e - s for s, e in chunks) / 1000

variants = {
    "default (beam 5)": [],
    f"audio_ctx {ac}": ["-ac", str(ac)],
    "beam 2": ["-bs", "2", "-bo", "2"],
    f"audio_ctx {ac} + beam 2": ["-ac", str(ac), "-bs", "2", "-bo", "2"],
    "greedy": ["-bs", "1", "-bo", "1"],
}
if len(sys.argv) > 4:
    variants = {k: v for k, v in variants.items() if any(x in k for x in sys.argv[4].split(","))}
print(f"{model}: {len(chunks)} chunks, {speech_s:.0f}s speech of {limit:.0f}s, longest chunk {longest/1000:.1f}s", flush=True)
for name, extra in variants.items():
    args = [f"{T}/whisper-cli", "-m", f"build/models/ggml-{model}.bin", "-l", lang, "-oj", "-ml", "1", "-sow", "-sns"]
    if lang in PROMPT:
        args += ["--prompt", PROMPT[lang]]
    args += extra
    for p in files:
        args += ["-f", str(p)]
    t0 = time.time()
    subprocess.run(args, capture_output=True)
    el = time.time() - t0
    text = ""
    for p in files:
        d = json.loads(open(str(p) + ".json", "rb").read().decode("utf-8", errors="replace"))
        text += "".join(s["text"] for s in d["transcription"])
    if lang == "bn":
        text = "".join(chr(ord(c) + 0x80) if 0x0900 <= ord(c) <= 0x097F and ord(c) not in (0x964, 0x965) else c for c in text)
    h = norm(text)
    cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
    print(f"  {name:28} CER {cer*100:5.1f}%   {el:6.1f}s   RTF {el/limit:.3f}", flush=True)
