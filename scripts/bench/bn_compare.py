#!/usr/bin/env python3
"""Developer check: Bangla models the way the app runs them (VAD chunks <= 12 s,
chunk-sized audio window, one whisper-cli run), with and without the Bangla prompt.
Scores WER (spelling: a misspelled word counts as wrong), CER and speed.

usage: bn_compare.py <minutes> <model> [<model> ...]
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
PROMPT = "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?"
minutes = float(sys.argv[1])
models = sys.argv[2:]
base = "build/testmedia/long bangla 30 min"
limit = minutes * 60
work = Path("build/samples/bncmp")
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
        o.setnchannels(1); o.setsampwidth(2); o.setframerate(rate)
        o.writeframes(frames[int(a * rate / 1000) * 2:int(b * rate / 1000) * 2])
    files.append(p)
longest = max(b - a for a, b in chunks) + 500
ac = min(1500, int(math.ceil(longest / 30_000 * 1500 / 64) * 64) + 64)
print(f"{len(chunks)} chunks over {limit:.0f}s; reference {len(ref.split())} words", flush=True)

rows = {}
for model in models:
    for use_prompt in (True, False):
        args = [f"{T}/whisper-cli", "-m", f"build/models/ggml-{model}.bin", "-l", "bn", "-oj", "-ml", "1", "-sow", "-sns", "-ac", str(ac)]
        if use_prompt:
            args += ["--prompt", PROMPT]
        for p in files:
            args += ["-f", str(p)]
        t0 = time.time()
        subprocess.run(args, capture_output=True)
        el = time.time() - t0
        text = ""
        for p in files:
            d = json.loads(open(str(p) + ".json", "rb").read().decode("utf-8", errors="replace"))
            text += " " + "".join(s["text"] for s in d["transcription"])
        text = "".join(chr(ord(c) + 0x80) if 0x0900 <= ord(c) <= 0x097F and ord(c) not in (0x964, 0x965) else c for c in text)
        h = norm(text)
        cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
        wer = edit_distance(h.split(), ref.split()) / len(ref.split())
        key = f"{model} {'prompt' if use_prompt else 'no prompt'}"
        rows[key] = {"wer": round(wer * 100, 1), "cer": round(cer * 100, 1), "rtf": round(el / limit, 3), "sample": text.strip()[:200]}
        print(f"  {key:40} WER {wer*100:5.1f}%  CER {cer*100:5.1f}%  RTF {el/limit:.3f}", flush=True)
Path("build/bench/bn-compare.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
