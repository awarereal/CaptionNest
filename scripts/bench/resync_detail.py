#!/usr/bin/env python3
"""Lists the captions an auto-sync run left more than 0.3 s from where they belong.

usage: resync_detail.py <good.srt> <resync-out.json>
"""
import json
import sys


def t(s):
    h, m, rest = s.split(":")
    sec, ms = rest.split(",")
    return (int(h) * 3600 + int(m) * 60 + int(sec)) * 1000 + int(ms)


truth = []
for block in open(sys.argv[1], encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
    lines = block.split("\n")
    a, b = lines[1].split(" --> ")
    truth.append((t(a), t(b), " ".join(lines[2:])))
res = json.load(open(sys.argv[2], encoding="utf-8"))
for i, (c, tr) in enumerate(zip(res["cues"], truth)):
    err = c["startMs"] - tr[0]
    if abs(err) > 300:
        print(f"#{i + 1:3} {c['status']:9} conf {c['confidence']:.2f}  truth {tr[0] / 1000:7.2f}-{tr[1] / 1000:7.2f}  was {c['oldStartMs'] / 1000:7.2f}"
              f"  now {c['startMs'] / 1000:7.2f}-{c['endMs'] / 1000:7.2f}  err {err:+5d}  {tr[2][:42]}")
