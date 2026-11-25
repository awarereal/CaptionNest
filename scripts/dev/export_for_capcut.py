#!/usr/bin/env python3
"""Generate captions for a video in the running app and export the SRT through the
Export dialog (used to prepare the CapCut import test)."""
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

video, out, spoken = sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "auto"
r = flow.generate(video, spoken=spoken, caption="same")
if os.path.exists(out):
    os.remove(out)
ui.click("Export SRT for CapCut")
ui.wait('return !!document.querySelector(".checks .check")', 20)
ui.queue(out)
ui.click("Export SRT…")
ui.wait('return !!document.querySelector(".export-done")', 20)
ui.eval_js('const b = [...document.querySelectorAll("button")].find(x => x.innerText.trim() === "Done"); b && b.click(); return true')
print(open(out, encoding="utf-8").read())
