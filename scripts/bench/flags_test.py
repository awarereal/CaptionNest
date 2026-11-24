#!/usr/bin/env python3
"""Developer check: which whisper-cli options help or hurt Bangla accuracy?

Runs the app's pipeline flags one at a time on a test video and scores CER against the
reference text.
"""
import json
import subprocess
import sys
import unicodedata
import re

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

T = "src-tauri/resources/tools"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
video = sys.argv[1]
ref_file = sys.argv[2]
model = sys.argv[3] if len(sys.argv) > 3 else "large-v3-q5_0"
lang = sys.argv[4] if len(sys.argv) > 4 else "bn"
PROMPTS = {"bn": "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?", "en": "Hello, and welcome. This is how we speak."}

subprocess.run([f"{T}/ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", video, "-ac", "1", "-ar", "16000",
                "-af", "aresample=async=1:first_pts=0", "-c:a", "pcm_s16le", "build/samples/ft.wav"], check=True)
ref = norm(open(ref_file, encoding="utf-8").read())

APP = ["--vad", "-vm", VAD, "-ml", "1", "-sow", "--prompt", PROMPTS[lang], "-sns"]
if "--decode" in sys.argv:
    variants = {
        "app": APP,
        "app -mc 0": APP + ["-mc", "0"],
        "app vmsd 15": APP + ["-vmsd", "15"],
        "app vmsd 10": APP + ["-vmsd", "10"],
        "app -mc 0 vmsd 15": APP + ["-mc", "0", "-vmsd", "15"],
        "app -nf": APP + ["-nf"],
        "app vsd 500": APP + ["-vsd", "500"],
    }
else:
    variants = {}
variants = variants or {
    "plain": [],
    "vad": ["--vad", "-vm", VAD],
    "words": ["-ml", "1", "-sow"],
    "prompt": ["--prompt", PROMPTS[lang]],
    "sns": ["-sns"],
    "app (all)": ["--vad", "-vm", VAD, "-ml", "1", "-sow", "--prompt", PROMPTS[lang], "-sns"],
    "app - vad": ["-ml", "1", "-sow", "--prompt", PROMPTS[lang], "-sns"],
    "app - prompt": ["--vad", "-vm", VAD, "-ml", "1", "-sow", "-sns"],
    "app - words": ["--vad", "-vm", VAD, "--prompt", PROMPTS[lang], "-sns"],
}
for name, extra in variants.items():
    subprocess.run([f"{T}/whisper-cli", "-m", f"build/models/ggml-{model}.bin", "-f", "build/samples/ft.wav", "-l", lang,
                    "-oj", "-of", "build/samples/ft"] + extra, capture_output=True)
    raw = open("build/samples/ft.json", "rb").read().decode("utf-8", errors="replace")
    d = json.loads(raw)
    hyp = "".join(s["text"] for s in d["transcription"])
    if lang == "bn":
        hyp = "".join(chr(ord(c) + 0x80) if 0x0900 <= ord(c) <= 0x097F and ord(c) not in (0x964, 0x965) else c for c in hyp)
    h = norm(hyp)
    cer = edit_distance(h.replace(" ", ""), ref.replace(" ", "")) / max(1, len(ref.replace(" ", "")))
    print(f"{name:14} CER {cer*100:5.1f}%  {len(d['transcription']):3} segs | {hyp.strip()[:110]}")
