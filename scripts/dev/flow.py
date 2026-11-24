#!/usr/bin/env python3
"""End-to-end flows against the running app (dev build with --features automation).

  python3 scripts/dev/flow.py generate <video> [--spoken bn] [--caption en] [--model id] [--shots prefix]
  python3 scripts/dev/flow.py home

Prints the resulting cues as JSON lines: {"start","end","text"}.
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import ui  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
SHOTS = os.path.join(ROOT, "build", "shots")


def shot(name):
    os.makedirs(SHOTS, exist_ok=True)
    path = os.path.join(SHOTS, f"{name}.png")
    ui.shot(path)
    return path


def go_home():
    ui.eval_js(
        """
        const st = window.__CN_STORE__;
        if (st) { st.setState({ dirty: false }); st.getState().closeProject(); }
        return true;
        """
    )
    ui.wait('return document.querySelector(".app").dataset.view === "home" && !!document.querySelector(".gate .btn--primary")', 15)
    time.sleep(0.4)


def set_settings(spoken=None, caption=None, model=None):
    patch = {}
    if spoken:
        patch["spokenLanguage"] = spoken
    if caption:
        patch["captionLanguage"] = caption
    if model:
        patch["transcribeModelId"] = model
    if patch:
        ui.eval_js(f"await window.__CN_STORE__.getState().updateSettings({json.dumps(patch)}); return true;")


def cues():
    return ui.eval_js(
        """
        return [...document.querySelectorAll('.cue')].map(c => {
          const t = [...c.querySelectorAll('.timefield')].map(x => x.value);
          return {start: t[0], end: t[1], text: c.querySelector('.cue__text').value};
        });
        """
    )


def all_cues():
    """Every cue of the active track, from the store (the list is virtualized)."""
    return ui.eval_js(
        """
        const s = window.__CN_STORE__.getState();
        const t = s.history.present.find(t => t.id === s.activeTrackId);
        return {language: t.language, kind: t.kind, tracks: s.history.present.map(x => x.language + ':' + x.kind),
                cues: t.cues.map(c => ({start: c.startMs, end: c.endMs, text: c.text}))};
        """
    )


def generate(video, spoken=None, caption=None, model=None, prefix=None, timeout=900):
    go_home()
    set_settings(spoken, caption, model)
    ui.queue(video)
    ui.click("Choose video")
    ui.wait('return !!document.querySelector(".prepare .options") || !!document.querySelector(".prepare .joberr")', 30)
    time.sleep(1.0)
    if prefix:
        shot(f"{prefix}-prepare")
    t0 = time.time()
    ui.click("Generate captions")
    ui.wait('return !!document.querySelector(".stage--active") || !!document.querySelector(".joberr") || !!document.querySelector(".ws")', 30)
    if prefix:
        time.sleep(1.5)
        shot(f"{prefix}-progress")
    ui.wait('return !!document.querySelector(".ws .cue") || !!document.querySelector(".joberr")', timeout)
    elapsed = time.time() - t0
    time.sleep(0.8)
    if prefix:
        shot(f"{prefix}-result")
    err = ui.eval_js('const e = document.querySelector(".joberr"); return e ? e.innerText : null')
    if err:
        return {"error": err, "seconds": round(elapsed, 1)}
    out = all_cues()
    out["seconds"] = round(elapsed, 1)
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd")
    ap.add_argument("video", nargs="?")
    ap.add_argument("--spoken")
    ap.add_argument("--caption")
    ap.add_argument("--model")
    ap.add_argument("--shots")
    a = ap.parse_args()
    if a.cmd == "home":
        go_home()
        print("home")
    elif a.cmd == "generate":
        print(json.dumps(generate(a.video, a.spoken, a.caption, a.model, a.shots), ensure_ascii=False, indent=1))
