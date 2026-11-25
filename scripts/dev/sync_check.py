#!/usr/bin/env python3
"""UI check: Auto-sync captions end to end, against exactly known times.

Opens the phrase recording (scripts/bench/make_phrase_audio.py: every phrase's start
and end is known exactly), loads its exact captions, knocks them 2 s late and replaces
one caption's words with words nobody says, then: Auto-sync → Find the right times →
preview → Apply. Checks the captions are back where they belong, the odd caption is
marked "Check timing", the text is untouched, and Undo restores the shifted times.
Also runs "Only a section".
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
flow.generate(os.path.join(ROOT, "build", "testmedia", "phrases", "bangla-phrases.wav"), spoken="bn", caption="same")
# Replace what was recognized with the exact captions of the recording.
exact = os.path.join(ROOT, "build", "testmedia", "phrases", "bangla-phrases.srt")
ui.eval_js(f'const cues = await window.__TAURI_INTERNALS__.invoke("import_srt", {{ path: {json.dumps(exact)} }}); window.__CN_STORE__.getState().replaceCues(cues); return cues.length')

CUES = ("const s = window.__CN_STORE__.getState(); const t = s.history.present.find(x => x.id === s.activeTrackId);"
        " return [...t.cues].sort((a, b) => a.startMs - b.startMs).map(c => [c.id, c.startMs, c.endMs, c.text, !!c.timingCheck])")
original = ui.eval_js(CUES)

# Knock everything 2 s late; make caption 3 say something that isn't in the audio.
ui.eval_js("window.__CN_STORE__.getState().offsetAll(2000); return true")
ui.eval_js(f'window.__CN_STORE__.getState().setCueText({json.dumps(original[2][0])}, "সম্পূর্ণ অন্য কিছু কথা এখানে লেখা।"); return true')
shifted = ui.eval_js(CUES)


def run_sync(shot_prefix, section=None):
    ui.click("Auto-sync")
    ui.wait('return !!document.querySelector("input[name=sync-scope]")', 10)
    if section:
        ui.eval_js('document.querySelectorAll("input[name=sync-scope]")[1].click(); return true')
        ui.wait('return !!document.querySelector(".syncrange")', 5)
        for label, value in (("Section start", section[0]), ("Section end", section[1])):
            ui.eval_js(
                f'const i = document.querySelector(\'input[aria-label="{label}"]\'); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;'
                f' set.call(i, "{value}"); i.dispatchEvent(new Event("input", {{bubbles: true}})); i.dispatchEvent(new FocusEvent("focusout", {{bubbles: true}})); return true'
            )
        time.sleep(0.4)
    time.sleep(0.4)
    flow.shot(f"{shot_prefix}-1-options")
    ui.click("Find the right times")
    ui.wait('return !!document.querySelector(".syncsum") || !!document.querySelector(".dl__error")', 300)
    time.sleep(0.5)
    flow.shot(f"{shot_prefix}-2-preview")
    return {
        "headline": ui.eval_js('return document.querySelector(".syncsum__head")?.innerText ?? null'),
        "counts": ui.eval_js('return document.querySelector(".syncsum .hint")?.innerText ?? null'),
        "rows": ui.eval_js('return [...document.querySelectorAll(".syncrow")].map(r => r.innerText.replace(/\\n/g, " | "))'),
    }


first = run_sync("sync")
ui.eval_js('[...document.querySelectorAll(".modal button")].find(b => /^Apply to/.test(b.innerText.trim())).click(); return true')
time.sleep(0.8)
flow.shot("sync-3-applied")
after = ui.eval_js(CUES)
chips = ui.eval_js('return [...document.querySelectorAll(".cue__check-label")].map(e => e.innerText.trim())')
ui.eval_js("window.__CN_STORE__.getState().undo(); return true")
time.sleep(0.3)
undone = ui.eval_js(CUES)

# Only a section: captions 1-4 stay 2 s late, the rest are synced.
cut = shifted[4][1] / 1000 - 0.3
second = run_sync("sync-section", section=(f"{cut:.2f}", "300"))
ui.eval_js('[...document.querySelectorAll(".modal button")].find(b => /^Apply to/.test(b.innerText.trim())).click(); return true')
time.sleep(0.6)
section_after = ui.eval_js(CUES)

err = [abs(a[1] - o[1]) for a, o in zip(after, original)]
report = {
    "headline": first["headline"],
    "counts": first["counts"],
    "start_error_ms_after_sync": err,
    "text_untouched": [a[3] for a in after] == [s[3] for s in shifted],
    "order_untouched": [a[0] for a in after] == [s[0] for s in shifted],
    "flagged_caption_3": after[2][4],
    "chips": chips,
    "undo_restores": [u[1:3] for u in undone] == [s[1:3] for s in shifted],
    "section_headline": second["headline"],
    "section_first_four_untouched": [a[1:3] for a in section_after[:4]] == [s[1:3] for s in shifted[:4]],
    "section_order_kept": [a[0] for a in section_after] == [s[0] for s in shifted],
    # The first caption of the section can't go where it's heard (the late caption
    # before it is in the way), so it stops there and is marked for review.
    "section_edge_caption_flagged": section_after[4][4],
    "section_rest_error_ms": [abs(a[1] - o[1]) for a, o in zip(section_after[5:], original[5:])],
}
others = sorted(e for i, e in enumerate(err) if i != 2)
report["start_error_ms_after_sync"] = {"median": others[len(others) // 2], "p90": others[len(others) * 9 // 10], "max": others[-1],
                                       "within_300ms": round(sum(e <= 300 for e in others) / len(others), 3)}
rest = sorted(report["section_rest_error_ms"])
report["section_rest_error_ms"] = {"median": rest[len(rest) // 2], "max": rest[-1]}
report["ok"] = (others[-1] <= 500 and report["start_error_ms_after_sync"]["within_300ms"] >= 0.9 and report["text_untouched"]
                and report["order_untouched"] and report["flagged_caption_3"] and report["undo_restores"]
                and report["section_first_four_untouched"] and report["section_order_kept"] and report["section_edge_caption_flagged"]
                and rest[-1] <= 500)
print(json.dumps(report, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["auto_sync_captions"] = report
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
