#!/usr/bin/env python3
"""Developer check: recognition speed vs spelling accuracy for the Bangla models, run
the way the app runs them (VAD chunks <= 12 s, chunk-sized audio window, two parallel
whisper-cli processes, DTW word timing), comparing decoding settings.

usage: [ONLY=variant|...] [PROCS=1,2,3] bn_speed.py <minutes> <model> [<model> ...]
"""
import json
import math
import re
import subprocess
import sys
import time
import wave
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

T = "src-tauri/resources/tools"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
PROMPT = "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?"
minutes = float(sys.argv[1])
models = sys.argv[2:]
base = "build/testmedia/long bangla 30 min"
limit = minutes * 60
work = Path("build/samples/bnspeed")
work.mkdir(parents=True, exist_ok=True)

subprocess.run([f"{T}/ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-ss", "300", "-i", base + ".mp4", "-t", str(limit),
                "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", str(work / "in.wav")], check=True)
timeline = json.load(open(base + " - timeline.json", encoding="utf-8"))
ref = norm(" ".join(u["text"] for u in timeline if u["start"] >= 300 and u["speech_end"] <= 300 + limit))

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
        o.setnchannels(1)
        o.setsampwidth(2)
        o.setframerate(rate)
        o.writeframes(frames[int(a * rate / 1000) * 2:int(b * rate / 1000) * 2])
    files.append(p)
longest = max(b - a for a, b in chunks) + 500
ac = min(1500, int(math.ceil(longest / 30_000 * 1500 / 64) * 64) + 64)
print(f"{len(chunks)} chunks over {limit:.0f}s; reference {len(ref.split())} words", flush=True)

DTW = {"large-v3-turbo-bn-q5_0": "large.v3.turbo", "large-v3-bn-q5_0": "large.v3", "large-v3-turbo-q5_0": "large.v3.turbo", "large-v3-q5_0": "large.v3"}
PROMPTED = {"large-v3-turbo-bn-q5_0"}
VARIANTS = {
    "beam 5, no DTW": lambda m: [],
    "now (beam 5, DTW)": lambda m: ["-dtw", DTW[m], "-nfa"],
    "greedy + DTW": lambda m: ["-bs", "1", "-bo", "1", "-dtw", DTW[m], "-nfa"],
    "beam 2 + DTW": lambda m: ["-bs", "2", "-bo", "2", "-dtw", DTW[m], "-nfa"],
    "greedy, no DTW": lambda m: ["-bs", "1", "-bo", "1"],
}

import os  # noqa: E402

ONLY = [v for v in os.environ.get("ONLY", "").split("|") if v]
PROCS = [int(p) for p in os.environ.get("PROCS", "2").split(",")]
rows = {}
for model, name, procs in [(m, n, p) for m in models for n in VARIANTS for p in PROCS if not ONLY or n in ONLY]:
    extra = VARIANTS[name]
    if True:
        common = [f"{T}/whisper-cli", "-m", f"build/models/ggml-{model}.bin", "-l", "bn", "-oj", "-ml", "1", "-sow", "-sns",
                  "-ac", str(ac), "-t", "4"] + extra(model)
        if model in PROMPTED:
            common += ["--prompt", PROMPT]
        groups = [files[i::procs] for i in range(procs)]
        t0 = time.time()
        with ThreadPoolExecutor(procs) as ex:
            list(ex.map(lambda g: subprocess.run(common + [x for p in g for x in ("-f", str(p))], capture_output=True), groups))
        el = time.time() - t0
        text = ""
        for p in files:
            d = json.loads(open(str(p) + ".json", "rb").read().decode("utf-8", errors="replace"))
            text += " " + "".join(s["text"] for s in d["transcription"])
        text = "".join(chr(ord(c) + 0x80) if 0x0900 <= ord(c) <= 0x097F and ord(c) not in (0x964, 0x965) else c for c in text)
        h = norm(text)
        cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
        wer = edit_distance(h.split(), ref.split()) / len(ref.split())
        key = f"{model} | {name} | {procs} proc"
        rows[key] = {"wer": round(wer * 100, 1), "cer": round(cer * 100, 1), "rtf": round(el / limit, 3)}
        print(f"  {key:52} WER {wer*100:5.1f}%  CER {cer*100:5.1f}%  RTF {el/limit:.3f}", flush=True)
Path("build/bench/bn-speed.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
