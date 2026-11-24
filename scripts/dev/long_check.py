#!/usr/bin/env python3
"""Developer check: run a long video through the real app and measure what came out.

usage: long_check.py "<video name without .mp4>" [model] [spoken]
Reports CER against the reference, how many utterances got no caption at all (missed
speech), empty "needs attention" captions, ordering/overlap problems and run time.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "bench"))
import flow  # noqa: E402
from benchmark_whisper import edit_distance, norm  # noqa: E402

name = sys.argv[1]
model = sys.argv[2] if len(sys.argv) > 2 else "large-v3-q5_0"
spoken = sys.argv[3] if len(sys.argv) > 3 else "auto"
base = os.path.join(flow.ROOT, "build", "testmedia", name)
res = flow.generate(base + ".mp4", spoken=spoken, caption="same", model=model, prefix=None, timeout=7200)
if "error" in res:
    print(json.dumps(res, ensure_ascii=False))
    sys.exit(1)
cues = res["cues"]
timeline = json.load(open(base + " - timeline.json", encoding="utf-8"))
ref = norm(open(base + " - reference.txt", encoding="utf-8").read())
hyp_raw = " ".join(c["text"] for c in cues)
hyp = norm(hyp_raw)
cer = edit_distance(hyp.replace(" ", ""), ref.replace(" ", "")) / len(ref.replace(" ", ""))
missed = [u for u in timeline if not any(c["start"] < u["speech_end"] * 1000 and c["end"] > u["start"] * 1000 and c["text"].strip() for c in cues)]
empty = [c for c in cues if not c["text"].strip()]
order_ok = all(a["start"] <= b["start"] for a, b in zip(cues, cues[1:]))
overlaps = sum(1 for a, b in zip(cues, cues[1:]) if b["start"] < a["end"])
duration = timeline[-1]["speech_end"]
report = {
    "video": name,
    "model": model,
    "seconds": res["seconds"],
    "realtime_factor": round(res["seconds"] / duration, 3),
    "cues": len(cues),
    "cer_percent": round(cer * 100, 1),
    "utterances": len(timeline),
    "utterances_without_caption": len(missed),
    "empty_review_cues": len(empty),
    "in_order": order_ok,
    "overlaps": overlaps,
    "last_cue_end_s": round(cues[-1]["end"] / 1000, 1) if cues else None,
    "speech_end_s": duration,
}
print(json.dumps(report, ensure_ascii=False, indent=1))
out = os.path.join(flow.ROOT, "build", "bench", f"long-{model}.json")
json.dump({"report": report, "missed": missed[:10]}, open(out, "w"), ensure_ascii=False, indent=1)
