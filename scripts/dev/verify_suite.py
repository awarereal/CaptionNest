#!/usr/bin/env python3
"""Verification suite: drives the real app (dev build with --features automation)
through the checks listed in docs/VERIFICATION.md and writes build/bench/verify.json.

usage: python3 scripts/dev/verify_suite.py [check ...]   (default: all)
"""
import json
import os
import shutil
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
MEDIA = os.path.join(ROOT, "build", "testmedia")
OUT = os.path.join(ROOT, "build", "verify")
os.makedirs(OUT, exist_ok=True)
results = {}


def media(name):
    return os.path.join(MEDIA, name)


def state(js):
    return ui.eval_js("const s = window.__CN_STORE__.getState();\n" + js)


def record(name, ok, **details):
    results[name] = {"ok": bool(ok), **details}
    print(("PASS " if ok else "FAIL ") + name, json.dumps(details, ensure_ascii=False)[:600], flush=True)


def first_cue_starts(cues):
    return [round(c["start"] / 1000, 2) for c in cues]


# ---------------------------------------------------------------- checks

def check_english():
    r = flow.generate(media("টেস্ট ভিডিও - English sample.mp4"), spoken="auto", caption="same", model="large-v3-q5_0", prefix="v-en")
    text = " ".join(c["text"] for c in r.get("cues", []))
    record("english_to_english", "Welcome" in text and "river." in text and r.get("language") == "en",
           seconds=r.get("seconds"), text=text, path_with_spaces_and_bengali=True)


def check_bangla_same():
    r = flow.generate(media("বাংলা নমুনা (Bangla sample).mp4"), spoken="bn", caption="same", model="large-v3-q5_0", prefix="v-bn")
    text = " ".join(c["text"] for c in r.get("cues", []))
    bengali = sum("ঀ" <= ch <= "৿" for ch in text) / max(1, sum(ch.isalpha() for ch in text))
    record("bangla_to_bangla", r.get("language") == "bn" and bengali > 0.95 and "হচ্ছে" in text,
           seconds=r.get("seconds"), bengali_script_share=round(bengali, 3), cues=len(r.get("cues", [])), text=text[:400])


def check_bangla_english():
    r = flow.generate(media("বাংলা নমুনা (Bangla sample).mp4"), spoken="auto", caption="en", model="large-v3-q5_0", prefix="v-bn2en")
    text = " ".join(c["text"] for c in r.get("cues", []))
    detected = state("return s.history.present[0].language")
    record("bangla_to_english", r.get("language") == "en" and detected == "bn" and "court" in text.lower() and "sclerosis" in text.lower(),
           seconds=r.get("seconds"), detected=detected, tracks=r.get("tracks"), text=text[:500])


def check_hindi_arabic():
    r = flow.generate(media("hindi sample.mp4"), spoken="auto", caption="same", model="large-v3-q5_0", prefix="v-hi")
    hi = " ".join(c["text"] for c in r.get("cues", []))
    record("hindi", r.get("language") == "hi" and any("ऀ" <= ch <= "ॿ" for ch in hi), text=hi)
    r = flow.generate(media("arabic sample.mp4"), spoken="auto", caption="same", model="large-v3-q5_0", prefix="v-ar")
    ar = " ".join(c["text"] for c in r.get("cues", []))
    direction = ui.eval_js('const t = document.querySelector(".cue__text"); return t ? [t.dir, getComputedStyle(t).direction] : null')
    ui.eval_js("const s = window.__CN_STORE__.getState(); const c = s.history.present[0].cues[0]; window.__cnSeek = c.startMs + 300; return true")
    ui.eval_js("const p = window.__CN_STORE__; return true")
    flow.shot("v-ar-rtl")
    record("arabic_rtl", r.get("language") == "ar" and any("؀" <= ch <= "ۿ" for ch in ar) and direction and direction[1] == "rtl",
           text=ar, textarea_direction=direction)


def check_timing():
    r = flow.generate(media("timing 0.5s 30s 56s.mp4"), spoken="en", caption="same", model="large-v3-q5_0", prefix="v-timing")
    starts = first_cue_starts(r.get("cues", []))
    texts = [c["text"] for c in r.get("cues", [])]
    expected = [0.5, 30.0, 56.0]
    ok = len(starts) == 3 and all(abs(a - b) <= 0.35 for a, b in zip(starts, expected))
    record("timing_start_middle_end", ok, starts=starts, expected=expected, texts=texts)

    r = flow.generate(media("delayed audio 2s.mp4"), spoken="en", caption="same", model="large-v3-q5_0", prefix="v-delay")
    starts = first_cue_starts(r.get("cues", []))
    record("audio_start_offset_2s", bool(starts) and abs(starts[0] - 2.0) <= 0.35, first_start=starts[:1], expected=2.0)

    r = flow.generate(media("variable frame rate.mp4"), spoken="en", caption="same", model="large-v3-q5_0", prefix="v-vfr")
    starts = first_cue_starts(r.get("cues", []))
    vfr = ui.eval_js("return window.__CN_STORE__.getState().media.video.variableFrameRate")
    record("variable_frame_rate", bool(starts) and starts[0] < 0.4 and "river" in " ".join(c["text"] for c in r["cues"]).lower(),
           detected_vfr=vfr, starts=starts)


def check_tracks_and_preview():
    flow.go_home()
    ui.queue(media("two tracks.mkv"))
    ui.click("Choose video")
    ui.wait('return !!document.querySelector(".prepare .options")', 30)
    tracks = state("return s.media.audioTracks.map(t => ({index: t.streamIndex, title: t.title, language: t.language}))")
    ui.eval_js("const s = window.__CN_STORE__.getState(); s.setAudioStream(s.media.audioTracks[1].streamIndex); return true")
    playable = state("return s.media.previewLikelyPlayable")
    ui.eval_js("await window.__CN_STORE__.getState().requestPreview(); return true")
    ui.wait('const p = window.__CN_STORE__.getState().preview; return p.status === "ready" || p.status === "failed" || p.status === "audio-only"', 120)
    preview = state("return {status: s.preview.status, url: s.preview.url}")
    time.sleep(1.5)
    flow.shot("v-preview")
    video_ok = ui.eval_js('const v = document.querySelector("video"); return v ? {ready: v.readyState, w: v.videoWidth, err: v.error && v.error.code} : null')
    flow.set_settings(spoken="bn", caption="same")
    ui.click("Generate captions")
    ui.wait('return !!document.querySelector(".ws .cue") || !!document.querySelector(".joberr")', 300)
    text = state("return s.history.present[0].cues.map(c => c.text).join(' ')")
    record("audio_track_choice_and_preview_copy",
           len(tracks) == 2 and not playable and preview["status"] == "ready" and video_ok and video_ok["w"] > 0
           and any("ঀ" <= ch <= "৿" for ch in text),
           tracks=tracks, preview=preview["status"], video=video_ok, text=text[:200])


def check_failures():
    r = flow.generate(media("silence only.mp4"), spoken="auto", caption="same", model="large-v3-q5_0", prefix="v-silence")
    record("silence_only_reports_no_speech", "No speech found" in (r.get("error") or ""), message=r.get("error"))

    flow.go_home()
    ui.queue(media("no audio.mp4"))
    ui.click("Choose video")
    ui.wait('return !!document.querySelector(".prepare .options")', 30)
    msg = ui.eval_js('const c = document.querySelector(".callout--danger"); return c ? c.innerText : null')
    disabled = ui.eval_js('return [...document.querySelectorAll("button")].find(b => b.innerText.includes("Generate captions")).disabled')
    flow.shot("v-noaudio")
    record("no_audio_explained_and_blocked", bool(msg) and "no sound" in msg.lower() and disabled, message=msg)

    flow.go_home()
    ui.queue(media("damaged.mp4"))
    ui.click("Choose video")
    time.sleep(2.5)
    toast = ui.eval_js('const t = document.querySelector("[data-sonner-toast]"); return t ? t.innerText : null')
    view = state("return s.view")
    record("damaged_file_explained", view == "home" and toast and "read" in toast.lower(), toast=toast)


def check_cancel_then_new_job():
    flow.go_home()
    flow.set_settings(spoken="bn", caption="en", model="large-v3-q5_0")
    ui.queue(media("long bangla 30 min.mp4"))
    ui.click("Choose video")
    ui.wait('return !!document.querySelector(".prepare .options")', 30)
    ui.click("Generate captions")
    ui.wait('const j = window.__CN_STORE__.getState().job; return j && j.stage === "transcribing" && (j.progress || 0) > 0.01', 180)
    first_job = state("return s.job.id")
    ui.click("Stop")
    ui.wait("return window.__CN_STORE__.getState().job === null", 30)
    after = state("return {view: s.view, error: s.jobError, project: !!s.project}")
    leftovers = os.listdir(os.path.join(ROOT, "build", "devhome", "cache", "jobs"))
    # A new job right after works and isn't overwritten by the stopped one.
    r = flow.generate(media("টেস্ট ভিডিও - English sample.mp4"), spoken="en", caption="same", model="large-v3-q5_0")
    record("cancel_then_new_job", after["view"] == "prepare" and not after["error"] and not after["project"] and not leftovers
           and "river" in " ".join(c["text"] for c in r.get("cues", [])).lower(),
           cancelled_job=first_job, state_after_cancel=after, scratch_left=leftovers)


def check_editing_save_export():
    r = flow.generate(media("টেস্ট ভিডিও - English sample.mp4"), spoken="en", caption="same", model="large-v3-q5_0")
    s0 = r["cues"]
    st = "const s = window.__CN_STORE__.getState(); const cues = () => window.__CN_STORE__.getState().history.present[0].cues;\n"
    steps = ui.eval_js(st + """
      const log = [];
      const ids = cues().map(c => c.id);
      s.setCueText(ids[0], "Welcome to CaptionNest!\\nSecond line kept.");  log.push(cues()[0].text);
      s.retimeCue(ids[1], 1800, 4100);                                   log.push([cues()[1].startMs, cues()[1].endMs]);
      const added = s.addCue(7000);                                       log.push(cues().length);
      s.setCueText(added, "Added caption");
      s.splitCue(ids[2], {caret: 15});                                    log.push(cues().length);
      s.mergeCue(ids[0]);                                                 log.push(cues().length);
      s.deleteCue(added);                                                 log.push(cues().length);
      s.offsetAll(250);                                                   log.push(cues()[0].startMs);
      s.undo(); s.undo();                                                 log.push(cues().length);
      s.redo();                                                           log.push(cues().length);
      return log;
    """)
    flow.shot("v-edited")
    proj = os.path.join(OUT, "সম্পাদনা project.captionnest")
    srt = os.path.join(OUT, "export test - English.srt")
    for p in (proj, srt):
        if os.path.exists(p):
            os.remove(p)
    ui.queue(proj)
    ui.eval_js('const {saveProject} = await import("/src/lib/fileFlows.ts"); return await saveProject(true)')
    saved = os.path.exists(proj)
    edited = ui.eval_js(st + "return cues()")
    # Reopen from disk.
    flow.go_home()
    ui.eval_js(f"await window.__CN_STORE__.getState().loadProject({json.dumps(proj)}); return true")
    reopened = ui.eval_js(st + "return cues()")
    same = [(c["startMs"], c["endMs"], c["text"]) for c in edited] == [(c["startMs"], c["endMs"], c["text"]) for c in reopened]
    # Relink: move the video, reopen, locate it.
    moved_dir = os.path.join(OUT, "moved")
    os.makedirs(moved_dir, exist_ok=True)
    src_video = media("টেস্ট ভিডিও - English sample.mp4")
    moved = os.path.join(moved_dir, "renamed video.mp4")
    shutil.copy(src_video, moved)
    data = json.load(open(proj, encoding="utf-8"))
    data["video"]["path"] = os.path.join(moved_dir, "gone.mp4")
    data["video"]["relativePath"] = None
    json.dump(data, open(proj, "w", encoding="utf-8"), ensure_ascii=False)
    flow.go_home()
    ui.eval_js(f"await window.__CN_STORE__.getState().loadProject({json.dumps(proj)}); return true")
    missing = state("return s.videoMissing")
    flow.shot("v-relink")
    ui.queue(moved)
    ui.click("Locate video")
    time.sleep(1.5)
    relinked = state("return {missing: s.videoMissing, path: s.project.video.path}")
    # Export through the export dialog.
    ui.eval_js('document.dispatchEvent(new KeyboardEvent("keydown", {key: "e", metaKey: true, bubbles: true})); return true')
    ui.click("Export SRT for CapCut")
    ui.wait('return !!document.querySelector(".checks .check")', 20)
    flow.shot("v-export-dialog")
    ui.queue(srt)
    clicked = ui.click("Export SRT…")
    ui.wait('return !!document.querySelector(".export-done")', 20)
    flow.shot("v-export-done")
    raw = open(srt, "rb").read()
    text = raw.decode("utf-8")
    blocks = text.split("\r\n\r\n")
    srt_ok = not raw.startswith(b"\xef\xbb\xbf") and "\r\n" in text and blocks[0].startswith("1\r\n") and " --> " in blocks[0] \
        and "Second line kept." in text
    ui.eval_js('const b = [...document.querySelectorAll("button")].find(x => x.innerText.trim() === "Done"); b && b.click(); return true')
    record("editing_undo_redo", steps[0] == "Welcome to CaptionNest!\nSecond line kept." and steps[1] == [1800, 4100], steps=steps)
    record("project_save_and_reopen", saved and same, project=proj, cues=len(reopened))
    record("relink_moved_video", missing and not relinked["missing"] and relinked["path"] == moved, relinked=relinked)
    record("export_srt_crlf_utf8", srt_ok, clicked=clicked, bytes=len(raw), first_block=blocks[0])


CHECKS = {
    "english": check_english,
    "bangla": check_bangla_same,
    "bangla_english": check_bangla_english,
    "hindi_arabic": check_hindi_arabic,
    "timing": check_timing,
    "tracks_preview": check_tracks_and_preview,
    "failures": check_failures,
    "cancel": check_cancel_then_new_job,
    "editing": check_editing_save_export,
}

if __name__ == "__main__":
    wanted = sys.argv[1:] or list(CHECKS)
    for name in wanted:
        try:
            CHECKS[name]()
        except Exception as e:  # noqa: BLE001
            record(name, False, exception=repr(e)[:500])
    path = os.path.join(ROOT, "build", "bench", "verify.json")
    prev = json.load(open(path)) if os.path.exists(path) else {}
    prev.update(results)
    json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
    print(f"{sum(r['ok'] for r in results.values())}/{len(results)} passed; wrote {path}")
