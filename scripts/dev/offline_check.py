#!/usr/bin/env python3
"""Offline check: with the app's internet routed to a dead proxy (HTTPS_PROXY=127.0.0.1:9),
captions must still be generated (English + Bangla) and translated; a model download
must fail with a clear message instead of hanging."""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
ui.wait('return !!window.__CN_STORE__ && !!document.querySelector(".app")', 120)
res = {}
en = flow.generate(os.path.join(ROOT, "build", "testmedia", "টেস্ট ভিডিও - English sample.mp4"), spoken="en", caption="same")
res["english"] = "river" in " ".join(c["text"] for c in en.get("cues", [])).lower()
bn = flow.generate(os.path.join(ROOT, "build", "testmedia", "বাংলা নমুনা (Bangla sample).mp4"), spoken="auto", caption="en")
res["bangla_to_english"] = bn.get("language") == "en" and "sclerosis" in " ".join(c["text"] for c in bn.get("cues", [])).lower()
# A download needs the internet: it must fail quickly with a readable message.
ui.eval_js('window.__CN_STORE__.getState().download("en_hi"); return true')
ui.wait('const d = window.__CN_STORE__.getState().downloads["en_hi"]; return d && d.state === "failed"', 90)
err = ui.eval_js('return window.__CN_STORE__.getState().downloads["en_hi"].error')
res["download_fails_clearly"] = err["code"] == "network"
res["download_message"] = err["message"]
print(json.dumps(res, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["offline_operation"] = {"ok": all(v for k, v in res.items() if k != "download_message"), **res}
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
