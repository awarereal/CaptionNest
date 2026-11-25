#!/usr/bin/env python3
"""Developer check: Sarvam speech-to-text (saaras:v4) on the same Bangla test set as
bn_compare.py (VAD speech chunks of the long FLEURS video), scored the same way.
Compares chunk lengths: 12 s (the app's offline setting) and 25 s (fewer requests).

usage: build/venv/bin/python scripts/bench/bn_sarvam.py <minutes>
The key comes from SARVAM_API_KEY or .env; it is never printed.
"""
import json
import os
import re
import subprocess
import sys
import time
import wave
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from sarvamai import SarvamAI

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

T = "src-tauri/resources/tools"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
minutes = float(sys.argv[1])
base = "build/testmedia/long bangla 30 min"
limit = minutes * 60
work = Path("build/samples/bnsarvam")
work.mkdir(parents=True, exist_ok=True)

key = os.environ.get("SARVAM_API_KEY") or next(
    (line.split("=", 1)[1].strip() for line in Path(".env").read_text().splitlines() if line.startswith("SARVAM_API_KEY=")), None
)
client = SarvamAI(api_subscription_key=key)

subprocess.run([f"{T}/ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-ss", "300", "-i", base + ".mp4", "-t", str(limit),
                "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", str(work / "in.wav")], check=True)
timeline = json.load(open(base + " - timeline.json", encoding="utf-8"))
ref = norm(" ".join(u["text"] for u in timeline if u["start"] >= 300 and u["speech_end"] <= 300 + limit))
out = subprocess.run([f"{T}/whisper-vad", "-f", str(work / "in.wav"), "-vm", VAD, "-np", "-vsd", "300", "-vp", "120", "-vmsd", "10"],
                     capture_output=True, text=True).stdout
segs = [(round(float(a) * 10), round(float(b) * 10)) for a, b in re.findall(r"start = ([\d.]+), end = ([\d.]+)", out)]
with wave.open(str(work / "in.wav")) as w:
    rate, frames = w.getframerate(), w.readframes(w.getnframes())


def call(path):
    for attempt in range(5):
        try:
            with open(path, "rb") as f:
                return client.speech_to_text.transcribe(file=f, model="saaras:v4", mode="transcribe", language_code="bn-IN").transcript
        except Exception as e:  # rate limit / overload: back off and retry
            if attempt == 4:
                raise
            time.sleep(2 ** attempt)


rows = {}
for max_ms in (12_000, 25_000):
    chunks = []
    for s, e in segs:
        if chunks and s - chunks[-1][1] < 1500 and e - chunks[-1][0] <= max_ms:
            chunks[-1] = (chunks[-1][0], e)
        else:
            chunks.append((s, e))
    files = []
    for i, (s, e) in enumerate(chunks):
        a, b = max(0, s - 250), e + 250
        p = work / f"m{max_ms}-c{i:04}.wav"
        with wave.open(str(p), "wb") as o:
            o.setnchannels(1)
            o.setsampwidth(2)
            o.setframerate(rate)
            o.writeframes(frames[int(a * rate / 1000) * 2:int(b * rate / 1000) * 2])
        files.append(p)
    t0 = time.time()
    with ThreadPoolExecutor(4) as ex:
        texts = list(ex.map(call, files))
    el = time.time() - t0
    h = norm(" ".join(texts))
    cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
    wer = edit_distance(h.split(), ref.split()) / len(ref.split())
    key_name = f"sarvam saaras:v4, chunks <= {max_ms // 1000} s"
    rows[key_name] = {"requests": len(files), "wer": round(wer * 100, 1), "cer": round(cer * 100, 1), "rtf": round(el / limit, 3),
                      "sample": " ".join(texts)[:200]}
    print(f"  {key_name:36} {len(files):3} requests  WER {wer*100:5.1f}%  CER {cer*100:5.1f}%  RTF {el/limit:.3f}", flush=True)
print(f"reference: {len(ref.split())} words over {limit:.0f}s")
Path("build/bench/bn-sarvam.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
