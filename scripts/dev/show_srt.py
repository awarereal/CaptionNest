#!/usr/bin/env python3
"""Print the captions of an SRT file compactly: start-end and text on one line.

usage: show_srt.py <file.srt> [count] [from_seconds]
"""
import sys

count = int(sys.argv[2]) if len(sys.argv) > 2 else 40
start_at = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0


def secs(stamp):
    h, m, rest = stamp.split(":")
    s, ms = rest.split(",")
    return int(h) * 3600 + int(m) * 60 + int(s) + int(ms) / 1000


shown = 0
for block in open(sys.argv[1], encoding="utf-8").read().replace("\r\n", "\n").strip().split("\n\n"):
    lines = block.split("\n")
    a, b = lines[1].split(" --> ")
    if secs(a) < start_at:
        continue
    print(f"{secs(a):7.2f}-{secs(b):7.2f}  {' / '.join(lines[2:])}")
    shown += 1
    if shown >= count:
        break
