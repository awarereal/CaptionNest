#!/usr/bin/env python3
"""Independent caption-timing check for any video (no ground truth needed).

For every caption, the audio of exactly its time window is cut out and recognized on
its own (Sarvam). If the caption is on screen while its words are spoken, that
transcript contains the caption's words. Coverage = share of the caption's letters
found, in order, in what was heard during its window. Captions shifted early or late
lose the words at one end and score low.

usage: build/venv/bin/python -I scripts/bench/cue_window_check.py <video> <file.srt> [max_seconds]
The key comes from SARVAM_API_KEY or .env; it is never printed.
"""
import difflib
import json
import os
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from sarvamai import SarvamAI

ROOT = Path(__file__).resolve().parents[2]
FFMPEG = str(ROOT / "src-tauri" / "target" / "release" / "tools" / "ffmpeg")
video, srt_path = sys.argv[1], sys.argv[2]
max_s = float(sys.argv[3]) if len(sys.argv) > 3 else 1e9
key = os.environ.get("SARVAM_API_KEY") or next(
    line.split("=", 1)[1].strip() for line in (ROOT / ".env").read_text().splitlines() if line.startswith("SARVAM_API_KEY=")
)
client = SarvamAI(api_subscription_key=key)


def t(s):
    h, m, rest = s.split(":")
    sec, ms = rest.split(",")
    return int(h) * 3600 + int(m) * 60 + int(sec) + int(ms) / 1000


def letters(s):
    # Numbers and money are written differently run to run ("₹50", "৫০ টাকা"): leave them out.
    s = s.replace("টাকা", "")
    return "".join(c for c in s if c.isalpha() or "ঀ" <= c <= "৥")


cues = []
for block in open(srt_path, encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
    lines = block.split("\n")
    a, b = lines[1].split(" --> ")
    if t(a) < max_s and letters(" ".join(lines[2:])):
        cues.append((t(a), t(b), " ".join(lines[2:])))

work = tempfile.mkdtemp(prefix="cuecheck-")
wav = os.path.join(work, "all.wav")
subprocess.run([FFMPEG, "-nostdin", "-v", "error", "-y", "-i", video, "-t", str(max_s + 40) if max_s < 1e8 else "86400",
                "-vn", "-ac", "1", "-ar", "16000", wav], check=True)


def heard(i):
    a, b, _ = cues[i]
    # Windows under 2 s are widened evenly: too short to recognize on their own.
    pad = max(0.0, (2.0 - (b - a)) / 2)
    part = os.path.join(work, f"c{i}.wav")
    subprocess.run([FFMPEG, "-nostdin", "-v", "error", "-y", "-ss", f"{max(0, a - pad):.3f}", "-t", f"{b - a + 2 * pad:.3f}", "-i", wav, part], check=True)
    for attempt in range(6):
        try:
            with open(part, "rb") as f:
                return client.speech_to_text.transcribe(file=f, model="saaras:v4", mode="transcribe", language_code="bn-IN").transcript or ""
        except Exception:  # rate limit: wait and retry
            time.sleep(3 * (attempt + 1))
    return ""


with ThreadPoolExecutor(3) as ex:
    texts = list(ex.map(heard, range(len(cues))))

rows = []
for (a, b, text), h in zip(cues, texts):
    want, got = letters(text), letters(h)
    m = difflib.SequenceMatcher(None, want, got, autojunk=False)
    cov = sum(blk.size for blk in m.get_matching_blocks()) / max(1, len(want))
    rows.append({"start": a, "end": b, "coverage": round(cov, 2), "caption": text, "heard": h.replace("\n", " ")})
covs = [r["coverage"] for r in rows]
weighted = sum(r["coverage"] * len(letters(r["caption"])) for r in rows) / max(1, sum(len(letters(r["caption"])) for r in rows))
summary = {
    "captions": len(rows),
    "mean_coverage": round(sum(covs) / max(1, len(covs)), 3),
    "letter_weighted_coverage": round(weighted, 3),
    "captions_at_least_80pct": round(sum(c >= 0.8 for c in covs) / max(1, len(covs)), 3),
    "captions_under_50pct": sum(c < 0.5 for c in covs),
}
print(json.dumps(summary))
for r in sorted(rows, key=lambda r: r["coverage"])[:8]:
    print(f"  {r['coverage']:.2f}  {r['start']:7.2f}-{r['end']:7.2f}  caption: {r['caption'][:60]}  | heard: {r['heard'][:60]}")
out = Path(srt_path).with_suffix(".windowcheck.json")
out.write_text(json.dumps({"summary": summary, "rows": rows}, ensure_ascii=False, indent=1), encoding="utf-8")
