#!/usr/bin/env python3
"""Auto-sync on real audio: well-timed captions are knocked out of sync on purpose,
auto-synced (headless, offline), and compared with where they were.

Cases: a steady offset, a growing drift, and single captions moved at random.
For each: what auto-sync reported, how far caption starts are from the original after
syncing, and how many captions it flagged instead of moving.

usage: resync_check.py <video> <good.srt> <language> [label]
"""
import json
import os
import random
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN = os.path.join(ROOT, "src-tauri", "target", "release", "captionnest")
video, good, lang = sys.argv[1], sys.argv[2], sys.argv[3]
label = sys.argv[4] if len(sys.argv) > 4 else "case"
out_dir = os.path.join(ROOT, "build", "bench", "resync")
os.makedirs(out_dir, exist_ok=True)


def t(s):
    h, m, rest = s.split(":")
    sec, ms = rest.split(",")
    return (int(h) * 3600 + int(m) * 60 + int(sec)) * 1000 + int(ms)


def stamp(ms):
    ms = max(0, int(ms))
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"


def read(path):
    cues = []
    for block in open(path, encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
        lines = block.split("\n")
        a, b = lines[1].split(" --> ")
        cues.append([t(a), t(b), "\n".join(lines[2:])])
    return cues


def write(path, cues):
    with open(path, "w", encoding="utf-8", newline="") as f:
        for i, (a, b, text) in enumerate(cues, 1):
            f.write(f"{i}\r\n{stamp(a)} --> {stamp(b)}\r\n" + text.replace("\n", "\r\n") + "\r\n\r\n")


truth = read(good)
random.seed(7)
picked = set(random.sample(range(len(truth)), max(1, len(truth) // 6)))
cases = {
    "offset +2.5 s": [[a + 2500, b + 2500, x] for a, b, x in truth],
    "drift 3% slow": [[a * 1.03, b * 1.03, x] for a, b, x in truth],
    "1 in 6 captions moved 1.5-4 s": [
        [a + d, b + d, x] for i, (a, b, x) in enumerate(truth)
        for d in [random.choice([-1, 1]) * random.randint(1500, 4000) if i in picked else 0]
    ],
}
env = dict(os.environ, CAPTIONNEST_HOME=os.path.join(ROOT, "build", "devhome"))
report = {}
only = os.environ.get("CASES")
for name, bad in cases.items():
    if only and name.split()[0] not in only.split(","):
        continue
    slug = f"{label}-{name.split()[0]}"
    bad_srt = os.path.join(out_dir, f"{slug}-in.srt")
    fixed_srt = os.path.join(out_dir, f"{slug}-out.srt")
    write(bad_srt, bad)
    subprocess.run([BIN, "--self-test", video, "--model", "unused", "--out", fixed_srt, "--language", lang, "--resync", bad_srt],
                   env=env, capture_output=True, check=False)
    res = json.load(open(fixed_srt[:-4] + ".json", encoding="utf-8"))
    if not res.get("ok"):
        report[name] = {"error": res.get("error")}
        print(name, report[name])
        continue
    cues = res["cues"]
    before = sorted(abs(b[0] - tr[0]) for b, tr in zip(bad, truth))
    moved = [(c, tr) for c, tr in zip(cues, truth) if c["status"] in ("matched", "estimated")]
    after = sorted(abs(c["startMs"] - tr[0]) for c, tr in moved)
    flagged = [c for c in cues if c["status"] in ("unmatched", "estimated")]
    worse = sum(1 for c, b, tr in zip(cues, bad, truth) if abs(c["startMs"] - tr[0]) > abs(b[0] - tr[0]) + 300)
    s = res["summary"]
    report[name] = {
        "seconds": round(res["seconds"], 1),
        "found": s["kind"], "first_shift_s": s["firstShiftMs"] / 1000, "last_shift_s": s["lastShiftMs"] / 1000,
        "captions": len(cues), "matched": s["matched"], "moved_with_neighbours": s["estimated"], "left_and_flagged": s["unmatched"],
        "start_error_before_ms": {"median": before[len(before) // 2], "max": before[-1]},
        "start_error_after_ms": {"median": after[len(after) // 2] if after else None, "p90": after[len(after) * 9 // 10] if after else None, "max": after[-1] if after else None},
        "within_300ms": round(sum(e <= 300 for e in after) / max(1, len(after)), 3),
        "made_worse": worse,
    }
    print(name, json.dumps(report[name]))
json.dump(report, open(os.path.join(out_dir, f"{label}.json"), "w"), indent=1)
