#!/usr/bin/env python3
"""Developer check: how often is real Bangla speech detected as Bangla?"""
import csv
import re
import subprocess
from collections import Counter
from pathlib import Path

T = "src-tauri/resources/tools/whisper-cli"
VAD = "src-tauri/resources/bundled-models/ggml-silero-v6.2.0.bin"
root = Path("build/fleurs")
files, seen = [], set()
for r in csv.reader(open(root / "bn_dev.tsv", encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE):
    if r[0] not in seen and (root / "dev" / r[1]).exists():
        seen.add(r[0])
        files.append(root / "dev" / r[1])
    if len(files) == 20:
        break
for model in ["large-v3-turbo-q5_0", "large-v3-q5_0"]:
    got, probs = Counter(), []
    for f in files:
        out = subprocess.run([T, "-m", f"build/models/ggml-{model}.bin", "-f", str(f), "-dl", "-l", "auto", "--vad", "-vm", VAD],
                             capture_output=True, text=True).stderr
        m = re.search(r"auto-detected language: (\w+) \(p = ([\d.]+)\)", out)
        if m:
            got[m.group(1)] += 1
            probs.append((m.group(1), float(m.group(2))))
    low = [p for l, p in probs if l == "bn"]
    print(model, dict(got), "bn p min/avg:", round(min(low), 2) if low else None, round(sum(low) / len(low), 2) if low else None,
          "wrong:", [(l, round(p, 2)) for l, p in probs if l != "bn"])
