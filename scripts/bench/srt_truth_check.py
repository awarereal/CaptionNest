#!/usr/bin/env python3
"""Text and timing of an SRT against ground truth (the long Bangla test video, whose
sentences and their exact start/end times are known).

Text: WER and CER of all caption text against the reference sentences.
Timing: every caption is matched to the sentence it shares most letters with; it is
"in place" when it starts no earlier than 0.3 s before that sentence starts and ends
no later than 0.5 s after it ends.

usage: srt_truth_check.py <file.srt> <timeline.json> <clip_start_seconds> <clip_length_seconds>
"""
import difflib
import json
import sys

sys.path.insert(0, "scripts/bench")
from benchmark_whisper import edit_distance, norm  # noqa: E402

srt_path, timeline_path, offset, length = sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4])


def t(s):
    h, m, rest = s.split(":")
    sec, ms = rest.split(",")
    return int(h) * 3600 + int(m) * 60 + int(sec) + int(ms) / 1000


def letters(s):
    return "".join(c for c in s if c.isalpha() or "ঀ" <= c <= "৥")


cues = []
for block in open(srt_path, encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
    lines = block.split("\n")
    a, b = lines[1].split(" --> ")
    cues.append((t(a), t(b), " ".join(lines[2:])))
utts = [(u["start"] - offset, u["speech_end"] - offset, u["text"]) for u in json.load(open(timeline_path, encoding="utf-8"))
        if u["start"] >= offset and u["speech_end"] <= offset + length]

# The clip may end inside a sentence: captions after the last whole sentence don't count.
last_end = max(u[1] for u in utts)
cues = [c for c in cues if c[0] < last_end + 0.5]
ref = norm(" ".join(u[2] for u in utts))
hyp = norm(" ".join(c[2] for c in cues))
wer = edit_distance(hyp.split(), ref.split()) / len(ref.split())
cer = edit_distance(hyp.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))

early, late, placed = [], [], 0
for a, b, text in cues:
    want = letters(text)
    if not want:
        continue
    # Sentences near the caption in time; the one sharing most letters is its sentence.
    near = [u for u in utts if u[1] > a - 20 and u[0] < b + 20]
    if not near:
        continue
    best = max(near, key=lambda u: sum(x.size for x in difflib.SequenceMatcher(None, want, letters(u[2]), autojunk=False).get_matching_blocks()))
    e, l = max(0.0, best[0] - a), max(0.0, b - best[1])
    early.append(e)
    late.append(l)
    placed += e <= 0.3 and l <= 0.5
    if e > 0.3 or l > 0.5:
        print(f"  off: caption {a:.2f}-{b:.2f} vs sentence {best[0]:.2f}-{best[1]:.2f}  early {e:.2f} late {l:.2f}  {text[:50]}", file=sys.stderr)
n = max(1, len(early))
print(json.dumps({
    "sentences": len(utts), "captions": len(cues),
    "wer": round(wer * 100, 1), "cer": round(cer * 100, 1),
    "captions_in_place": round(placed / n, 3),
    "worst_early_s": round(max(early, default=0), 2), "worst_late_s": round(max(late, default=0), 2),
    "mean_early_s": round(sum(early) / n, 3), "mean_late_s": round(sum(late) / n, 3),
}))
