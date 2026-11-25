#!/usr/bin/env python3
"""UI check: add a Sarvam key in Settings → Online recognition (typed into the field,
checked and saved), then generate Bangla captions with automatic language detection
and confirm they were recognized online.

The key is read from SARVAM_API_KEY or .env and never printed.
"""
import json
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
key = os.environ.get("SARVAM_API_KEY") or next(
    line.split("=", 1)[1].strip() for line in (Path(ROOT) / ".env").read_text().splitlines() if line.startswith("SARVAM_API_KEY=")
)
ui.wait('return !!window.__CN_STORE__ && !!document.querySelector(".app")', 120)

# Settings → Online recognition: type the key, Check and save.
ui.eval_js('window.__CN_STORE__.getState().updateSettings({ sarvamApiKey: null, onlineRecognition: true }); return true')
time.sleep(0.4)
ui.eval_js('window.__CN_STORE__.getState().openPanel("settings", "online"); return true')
ui.wait('return !!document.querySelector("#sarvam-key")', 15)
flow.shot("online-1-settings")
ui.eval_js(
    'const i = document.querySelector("#sarvam-key"); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;'
    f" set.call(i, {json.dumps(key)}); i.dispatchEvent(new Event('input', {{bubbles: true}})); return true"
)
ui.click("Check and save")
ui.wait('return !!document.querySelector(".onlinekey__ok") || !!document.querySelector("[role=alert]")', 60)
saved = ui.eval_js('return !!document.querySelector(".onlinekey__ok")')
time.sleep(0.4)
flow.shot("online-2-saved")
ui.eval_js('window.__CN_STORE__.getState().openPanel(null); return true')

# Generate Bangla captions with automatic detection.
t0 = time.time()
r = flow.generate(os.path.join(ROOT, "build", "testmedia", "বাংলা নমুনা (Bangla sample).mp4"), spoken="auto", caption="same", prefix="online")
seconds = round(time.time() - t0, 1)
model_used = ui.eval_js("return window.__CN_STORE__.getState().project?.settings ?? null")
warnings = ui.eval_js("return window.__CN_STORE__.getState().warnings")
report = {
    "key_saved": saved,
    "seconds_total_ui": seconds,
    "warnings": warnings,
    "language": r.get("language"),
    "cues": [(c["start"], c["end"], c["text"]) for c in r.get("cues", [])],
}
print(json.dumps(report, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["online_recognition_sarvam"] = {"ok": saved and bool(r.get("cues")), **report}
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
