#!/usr/bin/env python3
"""Print the summaries of cue_window_check.py result files, one per line.

usage: show_windowcheck.py <file.windowcheck.json> [...]
"""
import json
import os
import sys

for path in sys.argv[1:]:
    d = json.load(open(path, encoding="utf-8"))
    s = d["summary"]
    low = [r for r in d["rows"] if r["coverage"] < 0.5]
    print(f"{os.path.basename(os.path.dirname(path))}/{os.path.basename(path)}: captions {s['captions']}, "
          f"weighted coverage {s['letter_weighted_coverage']:.3f}, mean {s['mean_coverage']:.3f}, "
          f">=80%: {s['captions_at_least_80pct']:.0%}, under 50%: {len(low)}")
