#!/usr/bin/env python3
"""UI check: Shift timing for one caption using the difference to the video position.

Generates English captions offline, selects caption 2, moves the video 1.2 s past its
start, opens Shift timing, uses the worked-out difference for "Only 2", applies it and
checks that only that caption moved; then moves "2 and later" back and undoes.
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
ui.wait('return !!window.__CN_STORE__ && !!document.querySelector(".app")', 120)
ui.eval_js('window.__CN_STORE__.getState().updateSettings({ onlineRecognition: false }); return true')
flow.generate(os.path.join(ROOT, "build", "testmedia", "টেস্ট ভিডিও - English sample.mp4"), spoken="en", caption="same")


def times():
    return ui.eval_js("const s = window.__CN_STORE__.getState(); const t = s.history.present.find(x => x.id === s.activeTrackId);"
                      " return [...t.cues].sort((a, b) => a.startMs - b.startMs).map(c => [c.id, c.startMs, c.endMs])")


before = times()
second = before[1]
ui.eval_js(f'window.__CN_STORE__.getState().selectCue({json.dumps(second[0])}); return true')
ui.eval_js(f'const v = document.querySelector("video"); v.pause(); v.currentTime = {(second[1] + 1200) / 1000}; return true')
time.sleep(0.8)
ui.click("Shift timing")
ui.wait('return !!document.querySelector(".shift__diff")', 10)
time.sleep(0.3)
flow.shot("shift-1-popover")
label = ui.eval_js('return document.querySelector(".shift__diff button").innerText')
ui.eval_js('document.querySelector(".shift__diff button").click(); return true')
time.sleep(0.2)
scope = ui.eval_js('return document.querySelector(".shift__scope .is-on").innerText')
value = ui.eval_js('return document.querySelector("#shift-input").value')
flow.shot("shift-2-filled")
ui.eval_js('[...document.querySelectorAll(".shift button")].find(b => b.innerText.trim() === "Apply").click(); return true')
time.sleep(0.5)
after_one = times()

# "2 and later": move back by the same amount.
ui.click("Shift timing")
ui.wait('return !!document.querySelector(".shift__scope")', 10)
ui.eval_js('[...document.querySelectorAll(".shift__scope .seg__opt")][1].click(); return true')
ui.eval_js('const i = document.querySelector("#shift-input"); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;'
           ' set.call(i, "-0.5"); i.dispatchEvent(new Event("input", {bubbles: true})); return true')
time.sleep(0.2)
ui.eval_js('[...document.querySelectorAll(".shift button")].find(b => b.innerText.trim() === "Apply").click(); return true')
time.sleep(0.5)
after_from = times()
ui.eval_js("window.__CN_STORE__.getState().undo(); window.__CN_STORE__.getState().undo(); return true")
time.sleep(0.3)
restored = times()

shift = after_one[1][1] - before[1][1]
report = {
    "difference_button": label,
    "scope_when_caption_selected": scope,
    "value_filled": value,
    "only_caption_2_moved_ms": shift,
    "others_unchanged": after_one[0] == before[0] and after_one[2:] == before[2:],
    "from_2_on_moved": [a[1] - b[1] for a, b in zip(after_from, after_one)],
    "undo_restores_original": restored == before,
}
report["ok"] = (abs(shift - 1200) <= 40 and report["others_unchanged"] and report["from_2_on_moved"][0] == 0
                and all(d == -500 for d in report["from_2_on_moved"][1:]) and report["undo_restores_original"])
print(json.dumps(report, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["shift_timing_scopes"] = report
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
