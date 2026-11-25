#!/usr/bin/env python3
"""Print a CAPTIONNEST_DEBUG_DIR online-timing.json dump for a time range: the online
phrases (chunk spans), the offline timed words and where the online words were placed.

usage: show_timing_dump.py <online-timing.json> <from_seconds> <to_seconds>
"""
import json
import sys

d = json.load(open(sys.argv[1], encoding="utf-8"))
a, b = float(sys.argv[2]) * 1000, float(sys.argv[3]) * 1000


def line(rows):
    return " ".join(f"[{s / 1000:.2f}-{e / 1000:.2f} {t}]" for s, e, t in rows if e >= a and s <= b)


for s, e, t in d["phrases"]:
    if e >= a and s <= b:
        print(f"PHRASE {s / 1000:.2f}-{e / 1000:.2f}: {t}")
print("TIMED :", line(d["timed"]))
print("PLACED:", line(d["placed"]))
