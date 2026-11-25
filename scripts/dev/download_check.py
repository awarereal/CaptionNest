#!/usr/bin/env python3
"""UI check: download a model in Settings, stop part-way, resume, verify.
Run against the dev app (automation feature). Writes build/bench/verify.json entry."""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

MODEL = sys.argv[1] if len(sys.argv) > 1 else "large-v3-turbo-bn-q5_0"
ROOT = flow.ROOT
WDIR = os.path.join(ROOT, "build", "devhome", "data", "models", "whisper")


def dl():
    return ui.eval_js(f"const d = window.__CN_STORE__.getState().downloads[{json.dumps(MODEL)}]; return d || null")


def entry():
    return ui.eval_js(f"return window.__CN_STORE__.getState().models.whisper.find(m => m.id === {json.dumps(MODEL)})")


flow.go_home()
ui.eval_js('window.__CN_STORE__.getState().openPanel("settings", "models"); return true')
ui.wait('return !!document.querySelector(".settings__models .model")', 15)
card_js = f"""
const cards = [...document.querySelectorAll('.settings__models .model')];
const card = cards.find(c => c.querySelector('.model__name').innerText.trim() === {json.dumps('Bangla' if 'turbo' in MODEL else 'Bangla Max')});
return card;"""
ui.eval_js(card_js.replace("return card;", "card.scrollIntoView({block: 'center'}); return true;"))
flow.shot("dl-1-before")
ui.eval_js(card_js.replace("return card;", "const b = [...card.querySelectorAll('button')].find(x => x.innerText.includes('Download')); b.click(); return b.innerText;"))
t0 = time.time()
ui.wait(f"const d = window.__CN_STORE__.getState().downloads[{json.dumps(MODEL)}]; return d && d.total > 0 && d.received / d.total > 0.25", 600)
mid = dl()
flow.shot("dl-2-progress")
ui.eval_js(card_js.replace("return card;", "const b = [...card.querySelectorAll('button')].find(x => x.innerText.includes('Stop')); b.click(); return true;"))
ui.wait(f"const e = window.__CN_STORE__.getState().models.whisper.find(m => m.id === {json.dumps(MODEL)}); return e && !e.downloading && e.partialBytes > 0", 30)
time.sleep(2)
after_stop = entry()
part = os.path.join(WDIR, after_stop["file"] + ".part")
part_size = os.path.getsize(part) if os.path.exists(part) else 0
resume_label = ui.eval_js(card_js.replace("return card;", "const b = [...card.querySelectorAll('button')].find(x => x.innerText.includes('Resume')); return b ? b.innerText : null;"))
flow.shot("dl-3-stopped")
ui.eval_js(card_js.replace("return card;", "const b = [...card.querySelectorAll('button')].find(x => x.innerText.includes('Resume')); b.click(); return true;"))
first_after_resume = None
for _ in range(40):
    d = dl()
    if d and d.get("received"):
        first_after_resume = d["received"]
        break
    time.sleep(0.25)
ui.wait(f"const e = window.__CN_STORE__.getState().models.whisper.find(m => m.id === {json.dumps(MODEL)}); return e && e.installed", 1200)
elapsed = time.time() - t0
final = entry()
marker = open(os.path.join(WDIR, final["file"] + ".verified")).read().strip()
flow.shot("dl-4-installed")
ok = (part_size > 0 and resume_label and first_after_resume is not None and first_after_resume >= part_size * 0.99
      and final["installed"] and marker == final["sha256"] and not os.path.exists(part))
report = {
    "ok": bool(ok),
    "model": MODEL,
    "stopped_at_bytes": part_size,
    "resume_button": resume_label,
    "first_progress_after_resume": first_after_resume,
    "verified_sha256": marker == final["sha256"],
    "seconds_total": round(elapsed, 1),
}
print(json.dumps(report, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["model_download_stop_resume_verify"] = report
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
ui.eval_js('window.__CN_STORE__.getState().openPanel(null); return true')
