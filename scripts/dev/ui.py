#!/usr/bin/env python3
"""Drive the real CaptionNest app through its automation bridge (dev builds only).

Start the app with:
  CAPTIONNEST_AUTOMATION_PORT=7788 npm run tauri dev -- --features automation

Usage:
  python3 scripts/dev/ui.py eval 'return document.title'
  python3 scripts/dev/ui.py shot build/shots/home.png
  python3 scripts/dev/ui.py click 'Choose video'          # button by visible text
  python3 scripts/dev/ui.py queue '/path/to/file.mp4'      # answer the next file dialog
  python3 scripts/dev/ui.py wait 'return !!document.querySelector(".ws")' 60
"""
import json
import sys
import time
import urllib.request

PORT = 7788


def _post(path, body, timeout=320):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=body.encode("utf-8"), method="POST")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def eval_js(js):
    r = _post("/eval", js)
    if not r["ok"]:
        raise RuntimeError(r["value"])
    return json.loads(r["value"]) if isinstance(r["value"], str) else r["value"]


def shot(path):
    # Hidden windows pause CSS animations; show the final state for the snapshot.
    eval_js('document.documentElement.classList.add("no-motion"); return true')
    time.sleep(0.15)
    r = _post("/snapshot", path)
    if not r["ok"]:
        raise RuntimeError(r["value"])
    return path


def resize(width, height):
    """Resizes the app window (logical pixels)."""
    r = _post("/resize", f"{width}x{height}")
    if not r["ok"]:
        raise RuntimeError(r["value"])
    time.sleep(0.4)


def wait(js, timeout=60.0, every=0.5):
    end = time.time() + timeout
    while time.time() < end:
        try:
            if eval_js(js):
                return True
        except Exception:
            pass
        time.sleep(every)
    raise TimeoutError(js)


CLICK_JS = """
const want = %s;
const els = [...document.querySelectorAll('button, [role=tab], [role=menuitem], [role=option], [cmdk-item], a, label')];
const el = els.find(e => (e.innerText || e.getAttribute('aria-label') || '').trim() === want)
        || els.find(e => (e.innerText || e.getAttribute('aria-label') || '').trim().includes(want));
if (!el) throw new Error('no clickable element: ' + want);
if (el.disabled) throw new Error('disabled: ' + want);
el.scrollIntoView({block: 'center'});
for (const t of ['pointerdown','mousedown','pointerup','mouseup']) el.dispatchEvent(new PointerEvent(t, {bubbles: true, cancelable: true, pointerType: 'mouse', button: 0}));
el.click();
return (el.innerText || el.getAttribute('aria-label') || '').trim();
"""


def click(text):
    return eval_js(CLICK_JS % json.dumps(text))


def queue(path):
    return eval_js(
        "window.__CN_AUTOMATION__ = window.__CN_AUTOMATION__ || {paths: []};"
        f"window.__CN_AUTOMATION__.paths.push({json.dumps(path)}); return window.__CN_AUTOMATION__.paths.length;"
    )


if __name__ == "__main__":
    cmd, *args = sys.argv[1:]
    if cmd in ("js", "eval"):
        print(json.dumps(eval_js(args[0]), ensure_ascii=False, indent=1))
    elif cmd == "shot":
        print(shot(args[0]))
    elif cmd == "click":
        print(click(args[0]))
    elif cmd == "queue":
        print(queue(args[0]))
    elif cmd == "wait":
        print(wait(args[0], float(args[1]) if len(args) > 1 else 60))
    else:
        sys.exit(f"unknown command {cmd}")
