#!/usr/bin/env python3
"""Checks "Paste a video link" in the real app (automation build running).

Uses only freely licensed videos: Blender's Big Buck Bunny (CC-BY) on YouTube and on
Wikimedia Commons. Needs the video-link programs installed in CAPTIONNEST_HOME (the
first run of the dialog sets them up). Saves into build/linkclips/.

usage: link_check.py [read|part|stop|background|errors|commons|all]
"""
import os
import re
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import ui  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(ROOT, "build", "linkclips")
SHOTS = os.path.join(ROOT, "build", "shots")
YT = "https://www.youtube.com/watch?v=aqz-KE-bpKQ&t=90"
COMMONS = "https://commons.wikimedia.org/wiki/File:Big_Buck_Bunny_4K.webm"

failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  ({detail})" if detail else ""), flush=True)
    if not ok:
        failures.append(name)


def js(code):
    return ui.eval_js(code)


def setup():
    # Start empty: an existing file of the same name is reused, which would skip the
    # download being checked.
    shutil.rmtree(OUT, ignore_errors=True)
    os.makedirs(OUT, exist_ok=True)
    js(f"""window.__CN_STORE__.getState().updateSettings({{ clipFolder: {OUT!r} }});
           window.__CN_STORE__.getState().setView("home"); return true""")


def open_link(url):
    js(f"""const f = document.querySelector('.linkbar__input');
           const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
           set.call(f, {url!r}); f.dispatchEvent(new Event('input', {{bubbles: true}})); f.form.requestSubmit(); return true""")


def phase():
    return js("""const t = document.querySelector('.linkdialog');
                 if (!t) return 'closed';
                 if (document.querySelector('.linkfail')) return 'failed';
                 if (document.querySelector('.linkfetch')) return 'fetching';
                 if (document.querySelector('.linkcard--loading')) return 'reading';
                 if (document.querySelector('.linkcard')) return 'ready';
                 if (document.querySelector('.linksetup')) return 'setup';
                 return 'idle'""")


def wait_phase(want, timeout=180):
    end = time.time() + timeout
    while time.time() < end:
        p = phase()
        if p in want:
            return p
        time.sleep(0.5)
    return phase()


def set_time(field_id, text):
    js(f"""const i = document.querySelector('#{field_id}');
           i.focus();
           const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
           set.call(i, {text!r}); i.dispatchEvent(new Event('input', {{bubbles: true}}));
           i.dispatchEvent(new KeyboardEvent('keydown', {{key: 'Enter', bubbles: true}})); return true""")
    time.sleep(0.3)
    got = js(f"return document.querySelector('#{field_id}').value")
    check(f"{field_id} set to {text}", got == text, got)


def close_dialog():
    js("""document.querySelector('.linkdialog .modal__head button[aria-label=Close]')?.click(); return true""")
    time.sleep(0.5)


def shot(name):
    js("document.getAnimations().forEach(a => a.finish()); return true")
    ui.shot(os.path.join(SHOTS, name))


def ytdlp_running():
    r = subprocess.run(["pgrep", "-f", "link-tools/yt-dlp/yt-dlp_macos"], capture_output=True, text=True)
    f = subprocess.run(["pgrep", "-f", "resources/tools/ffmpeg"], capture_output=True, text=True)
    return bool(r.stdout.strip()), bool(f.stdout.strip())


def t_read():
    open_link(YT)
    t0 = time.time()
    p = wait_phase({"ready", "failed"})
    check("YouTube link is read", p == "ready", f"{time.time() - t0:.1f} s")
    sizes = js("""return [...document.querySelectorAll('.linkq .radio-card')].map(e => e.innerText.replace(/\\n/g, ' | '))""")
    print("   ", sizes)
    audio = [s for s in sizes if s.startswith("Sound only")]
    m = re.search(r"about ([\d.]+) (KB|MB)", audio[0]) if audio else None
    mb = (float(m.group(1)) / (1000 if m.group(2) == "KB" else 1)) if m else 0.0
    check("sound-only estimate for 1 min is about 1 MB", 0.6 <= mb <= 1.6, audio[0] if audio else "")
    ui.wait("return document.querySelectorAll('.trim__frames .frame.is-loaded').length >= 4", 20)
    frames = js("return document.querySelectorAll('.trim__frames .frame.is-loaded').length")
    check("preview frames load", frames >= 4, f"{frames} frames")
    t = js("return [document.querySelector('#link-from').value, document.querySelector('#link-to').value]")
    check("link's t=90 becomes the start", t == ["1:30", "2:30"], str(t))
    shot("link-ready-2.png")


def t_stop():
    # A long part, highest quality, so it's still running when stopped.
    set_time("link-to", "9:00")
    ui.click("Highest quality")
    ui.click("Get this part")
    check("download starts", wait_phase({"fetching"}, 20) == "fetching")
    time.sleep(6)
    before = ytdlp_running()
    ui.click("Stop")
    p = wait_phase({"ready", "failed"}, 30)
    time.sleep(1.5)
    after = ytdlp_running()
    check("Stop returns to the choices", p == "ready", p)
    check("Stop ends yt-dlp and ffmpeg", after == (False, False), f"running before {before}, after {after}")
    left = [f for f in os.listdir(OUT) if "1m30s-9m00s" in f]
    check("nothing half-finished is left in the folder", not left, str(left))


def t_background():
    set_time("link-to", "2:00")
    ui.click("For editing")
    ui.click("Get this part")
    wait_phase({"fetching"}, 20)
    time.sleep(2)
    close_dialog()
    toast = js("return document.querySelector('[data-sonner-toast]')?.innerText ?? null")
    check("closing during a download leaves a notice", bool(toast) and "Getting the video" in toast, str(toast))
    shot("link-background.png")
    ok = ui.wait('return (window.__CN_STORE__.getState().media?.path ?? "").includes("1m30s-2m00s")', 240)
    path = js("return window.__CN_STORE__.getState().media?.path")
    check("the part opens for captions when it's done", bool(ok) and path and "1m30s-2m00s" in path, str(path))
    dur = js("return window.__CN_STORE__.getState().media?.durationMs")
    check("its length is the part's", dur and abs(dur - 30000) < 600, str(dur))


def t_errors():
    js('window.__CN_STORE__.getState().setView("home"); return true')
    time.sleep(0.5)
    for url, want in [
        ("https://www.youtube.com/watch?v=xxxxxxxxxxx", "isn't available"),
        ("https://example.com/", "can't get a video"),
        ("not a link", "doesn't look like a video link"),
    ]:
        open_link(url)
        p = wait_phase({"failed", "ready"}, 120)
        msg = js("return document.querySelector('.linkfail__msg')?.innerText ?? ''")
        check(f"error for {url!r} is plain", p == "failed" and want in msg, msg)
        close_dialog()
    shot("link-error.png")


def t_commons():
    js('window.__CN_STORE__.getState().setView("home"); return true')
    time.sleep(0.5)
    open_link(COMMONS)
    p = wait_phase({"ready", "failed"})
    meta = js("return [document.querySelector('.linkcard__title')?.innerText, document.querySelector('.linkcard__meta')?.innerText, document.querySelector('.linkfail__msg')?.innerText]")
    check("a link from another site (Wikimedia Commons) is read", p == "ready", str(meta))
    if p != "ready":
        return
    ui.click("Only a part")
    set_time("link-to", "0:20")
    shot("link-commons.png")
    ui.click("Get this part")
    ui.wait('return (window.__CN_STORE__.getState().media?.path ?? "").includes("0s-20s") || !!document.querySelector(".linkfail")', 300)
    path = js("return window.__CN_STORE__.getState().media?.path")
    err = js("return document.querySelector('.linkfail__msg')?.innerText ?? null")
    check("its part is fetched and opened", bool(path) and "0s-20s" in str(path), str(path or err))


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    ui.wait("return !!window.__CN_STORE__ && !!document.querySelector('.app')", 120)
    setup()
    steps = {"read": [t_read], "stop": [t_read, t_stop], "background": [t_read, t_background], "errors": [t_errors], "commons": [t_commons]}
    for step in steps.get(which, [t_read, t_stop, t_background, t_errors, t_commons]):
        step()
    print("\n" + ("ALL PASSED" if not failures else f"{len(failures)} FAILED: {failures}"))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
