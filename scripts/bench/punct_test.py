#!/usr/bin/env python3
"""Developer check: does an initial prompt restore punctuation in timestamped decoding?"""
import json
import subprocess
import sys

T = "src-tauri/resources/tools/whisper-cli"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
cases = [
    ("en", "build/samples/en.wav", "Punctuation matters: commas, periods, and capital letters."),
    ("bn", "build/samples/bn_say.wav", "আমি বাংলায় কথা বলি। আজ আবহাওয়া ভালো, তাই না?"),
]
models = sys.argv[1:] or ["large-v3-q5_0", "large-v3-turbo-q5_0"]
for model in models:
    for lang, wav, prompt in cases:
        for use_prompt in (False, True):
            args = [T, "-m", f"build/models/ggml-{model}.bin", "-f", wav, "-l", lang, "-oj", "-of", "build/samples/pt",
                    "-ml", "1", "-sow", "-sns", "--vad", "-vm", VAD]
            if use_prompt:
                args += ["--prompt", prompt]
            subprocess.run(args, capture_output=True)
            d = json.load(open("build/samples/pt.json", encoding="utf-8", errors="replace"))
            text = "".join(s["text"] for s in d["transcription"]).strip()
            print(f"{model:22} {lang} prompt={use_prompt!s:5} | {text[:150]}")
