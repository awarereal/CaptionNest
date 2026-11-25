#!/usr/bin/env python3
"""UI check: English captions, then Add language → Bangla through an Argos pack that is
downloaded in Settings. Verifies the new track keeps every caption's timing."""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
video = os.path.join(ROOT, "build", "testmedia", "টেস্ট ভিডিও - English sample.mp4")
r = flow.generate(video, spoken="en", caption="same")
original = r["cues"]

# Install the English → Bangla pack through Settings → Language packs.
installed = ui.eval_js('return !!window.__CN_STORE__.getState().models.argos.find(p => p.id === "en_bn" && p.installed)')
if not installed:
    ui.eval_js('window.__CN_STORE__.getState().openPanel("settings", "languages"); return true')
    ui.wait('return !!document.querySelector(".packs .pack")', 15)
    ui.eval_js('const i = document.querySelector(".search__input"); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set; set.call(i, "Bangla"); i.dispatchEvent(new Event("input", {bubbles: true})); return true')
    time.sleep(0.5)
    flow.shot("tr-1-packs")
    ui.eval_js("""
      const row = [...document.querySelectorAll('.pack')].find(r => /English\\s*→\\s*Bangla/.test(r.innerText));
      [...row.querySelectorAll('button')].find(b => b.innerText.includes('Download')).click();
      return true;""")
    ui.wait('return !!window.__CN_STORE__.getState().models.argos.find(p => p.id === "en_bn" && p.installed)', 600)
    flow.shot("tr-2-installed")
    ui.eval_js('window.__CN_STORE__.getState().openPanel(null); return true')

# Add language → Bangla
ui.click("Add language")
ui.wait('return !!document.querySelector("#tr-target")', 10)
ui.eval_js('document.querySelector("#tr-target").click(); return true')
ui.wait('return !!document.querySelector("[cmdk-input]")', 5)
ui.eval_js('const i = document.querySelector("[cmdk-input]"); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set; set.call(i, "Bangla"); i.dispatchEvent(new Event("input", {bubbles: true})); return true')
time.sleep(0.5)
ui.eval_js('const it = [...document.querySelectorAll("[cmdk-item]")].find(x => x.innerText.includes("Bangla")); it.click(); return it.innerText')
time.sleep(0.5)
flow.shot("tr-3-dialog")
t0 = time.time()
ui.click("Translate")
ui.wait('const s = window.__CN_STORE__.getState(); return !s.job && s.history.present.some(t => t.language === "bn")', 300)
elapsed = time.time() - t0
time.sleep(0.8)
flow.shot("tr-4-result")
tracks = ui.eval_js("return window.__CN_STORE__.getState().history.present.map(t => ({lang: t.language, engine: t.engine, cues: t.cues.map(c => [c.startMs, c.endMs, c.text])}))")
en = next(t for t in tracks if t["lang"] == "en")
bn = next(t for t in tracks if t["lang"] == "bn")
same_timing = [c[:2] for c in en["cues"]] == [c[:2] for c in bn["cues"]]
bengali = all(any("ঀ" <= ch <= "৿" for ch in c[2]) for c in bn["cues"])
report = {"ok": same_timing and bengali and bn["engine"] == "argos", "seconds": round(elapsed, 1), "same_timing": same_timing,
          "english": [c[2] for c in en["cues"]], "bangla": [c[2] for c in bn["cues"]]}
print(json.dumps(report, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["english_to_bangla_offline_translation"] = report
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
