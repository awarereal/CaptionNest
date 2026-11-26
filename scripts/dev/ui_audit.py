#!/usr/bin/env python3
"""UI audit: visits every screen, panel and dialog at the normal and the minimum window
size, takes a snapshot of each, and checks the layout automatically:
  - text cut off inside its box (clipped without an ellipsis),
  - anything sticking out of the window,
  - buttons and fields overlapping each other,
  - interactive elements without an accessible name,
  - images and icons with zero size,
  - console errors.

usage: ui_audit.py [label] [theme] [sizes]   (app running with the automation bridge)
  theme: a theme id from src/lib/theme.ts (default: leave as is)
  sizes: e.g. "1360x860,900x600" (the default)
Writes build/shots/audit/<label>-<state>.png and build/bench/ui-audit-<label>.json.
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
LABEL = sys.argv[1] if len(sys.argv) > 1 else "audit"
THEME = sys.argv[2] if len(sys.argv) > 2 else None
SIZE_ARG = sys.argv[3] if len(sys.argv) > 3 else "1360x860,900x600"
SHOTS = os.path.join(ROOT, "build", "shots", "audit")
os.makedirs(SHOTS, exist_ok=True)

CHECK_JS = r"""
const issues = [];
const W = window.innerWidth, H = window.innerHeight;
const visible = (el) => {
  if (el.closest && el.closest(".visually-hidden")) return false;
  const r = el.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) return false;
  const cs = getComputedStyle(el);
  if (cs.visibility === "hidden" || cs.display === "none" || Number(cs.opacity) === 0) return false;
  for (let p = el; p; p = p.parentElement) {
    const s = getComputedStyle(p);
    if (s.display === "none" || s.visibility === "hidden" || Number(s.opacity) === 0) return false;
    if (p.hasAttribute && p.hasAttribute("inert")) return false;
  }
  return true;
};
const label = (el) => {
  const t = (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("placeholder") || "").trim().replace(/\s+/g, " ");
  const cls = (el.className && typeof el.className === "string") ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
  return `${el.tagName.toLowerCase()}${cls} "${t.slice(0, 50)}"`;
};
// The layer the user is looking at: the open dialog or popover, else the page.
const layers = [...document.querySelectorAll("[role=dialog], [data-radix-popper-content-wrapper], [role=menu], [role=listbox]")].filter(visible);
const scope = layers.length ? layers : [document.querySelector(".app") || document.body];
const all = scope.flatMap((s) => [s, ...s.querySelectorAll("*")]);

// 1. Text cut off: overflowing its box horizontally with no ellipsis, or vertically.
for (const el of all) {
  if (!visible(el)) continue;
  const cs = getComputedStyle(el);
  const tag = el.tagName;
  if (["TEXTAREA", "INPUT", "SELECT", "CANVAS", "VIDEO", "SVG", "svg", "PATH", "path", "IMG"].includes(tag)) continue;
  const hasText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
  if (!hasText) continue;
  const clipX = ["hidden", "clip"].includes(cs.overflowX) || ["hidden", "clip"].includes(cs.overflow);
  const clipY = ["hidden", "clip"].includes(cs.overflowY) || ["hidden", "clip"].includes(cs.overflow);
  if (clipX && el.scrollWidth > el.clientWidth + 2 && cs.textOverflow !== "ellipsis") issues.push(["clipped-text", label(el), `${el.scrollWidth}>${el.clientWidth}`]);
  if (clipY && el.scrollHeight > el.clientHeight + 3 && !cs.webkitLineClamp?.match?.(/\d/) && cs.webkitLineClamp !== undefined && cs.webkitLineClamp === "none") issues.push(["clipped-text-y", label(el), `${el.scrollHeight}>${el.clientHeight}`]);
}

// 2. Sticking out of the window (the parts not in a scroll area).
const inScroller = (el) => {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const s = getComputedStyle(p);
    if (/(auto|scroll|hidden|clip)/.test(s.overflowX + s.overflowY + s.overflow) && p !== document.body && p !== document.documentElement) return true;
  }
  return false;
};
for (const el of all) {
  if (!visible(el) || inScroller(el)) continue;
  const r = el.getBoundingClientRect();
  if (r.right > W + 1 || r.bottom > H + 1 || r.left < -1 || r.top < -1) issues.push(["off-window", label(el), `${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)} in ${W}x${H}`]);
}
if (document.documentElement.scrollWidth > W + 1) issues.push(["page-scrolls-sideways", "html", `${document.documentElement.scrollWidth}>${W}`]);

// 3. Interactive elements overlapping each other (same layer, both visible).
const controls = all.filter((el) => el.matches?.("button, a, input, textarea, select, [role=radio], [role=tab], [role=menuitem], [role=switch]") && visible(el));
// The part of an element that can be seen: its box cut by every scrolling or clipping parent.
const seen = (el) => {
  let r = el.getBoundingClientRect();
  let box = { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
  for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
    const s = getComputedStyle(p);
    if (/(auto|scroll|hidden|clip)/.test(s.overflow + s.overflowX + s.overflowY)) {
      const q = p.getBoundingClientRect();
      box = { left: Math.max(box.left, q.left), top: Math.max(box.top, q.top), right: Math.min(box.right, q.right), bottom: Math.min(box.bottom, q.bottom) };
    }
  }
  return box;
};
const hit = (el) => {
  const r = el.getBoundingClientRect();
  const x = Math.min(Math.max(r.left + r.width / 2, 0), W - 1), y = Math.min(Math.max(r.top + r.height / 2, 0), H - 1);
  const top = document.elementFromPoint(x, y);
  return !top || el === top || el.contains(top) || top.contains(el);
};
for (let i = 0; i < controls.length; i++) {
  const a = controls[i];
  if (a.closest("[data-sonner-toaster]")) continue;
  const ra = a.getBoundingClientRect();
  if (!hit(a)) {
    const top = document.elementFromPoint(ra.left + ra.width / 2, ra.top + ra.height / 2);
    if (top && !top.closest("[data-sonner-toaster]") && !inScroller(a)) issues.push(["covered", label(a), "under " + label(top)]);
  }
  for (let j = i + 1; j < controls.length; j++) {
    const b = controls[j];
    if (a.contains(b) || b.contains(a)) continue;
    const [sa, sb] = [seen(a), seen(b)];
    const ox = Math.min(sa.right, sb.right) - Math.max(sa.left, sb.left);
    const oy = Math.min(sa.bottom, sb.bottom) - Math.max(sa.top, sb.top);
    // A button placed inside a text box's reserved padding (the mic) is by design.
    const inPadding = (box, btn) => box.matches("textarea, input") && parseFloat(getComputedStyle(box).paddingRight) >= btn.getBoundingClientRect().width;
    // Content scrolling under a footer that stays pinned (position: sticky) is by design.
    const sticky = (el) => { for (let p = el; p; p = p.parentElement) if (getComputedStyle(p).position === "sticky") return true; return false; };
    if (ox > 3 && oy > 3 && !inPadding(a, b) && !inPadding(b, a) && sticky(a) === sticky(b)) issues.push(["controls-overlap", label(a), label(b)]);
  }
}

// 4. Interactive elements without a name.
for (const el of controls) {
  const name = (el.getAttribute("aria-label") || el.innerText || el.value || el.getAttribute("title") || el.getAttribute("placeholder") || (el.labels && el.labels[0] && el.labels[0].innerText) || (el.getAttribute("aria-labelledby") && document.getElementById(el.getAttribute("aria-labelledby"))?.innerText) || "").trim();
  if (!name && el.type !== "hidden") issues.push(["no-name", label(el), el.outerHTML.slice(0, 90)]);
}

// 5. Icons and images with no size.
for (const el of all) {
  if (!["svg", "IMG"].includes(el.tagName) || !el.closest("button, a, .app")) continue;
  const r = el.getBoundingClientRect();
  const parentVisible = el.parentElement && visible(el.parentElement);
  if (parentVisible && (r.width < 1 || r.height < 1)) issues.push(["zero-size-icon", label(el.parentElement), ""]);
}
return { size: `${W}x${H}`, issues: issues.slice(0, 60), count: issues.length };
"""

# Collect console errors from now on.
ui.eval_js("""
window.__AUDIT_ERRORS__ = window.__AUDIT_ERRORS__ || [];
if (!window.__AUDIT_HOOKED__) {
  window.__AUDIT_HOOKED__ = true;
  const e = console.error.bind(console);
  console.error = (...a) => { window.__AUDIT_ERRORS__.push(a.map(String).join(" ").slice(0, 300)); e(...a); };
  window.addEventListener("error", (ev) => window.__AUDIT_ERRORS__.push("uncaught: " + ev.message));
  window.addEventListener("unhandledrejection", (ev) => window.__AUDIT_ERRORS__.push("rejection: " + String(ev.reason).slice(0, 300)));
}
return true;""")

report = {}


def settle():
    time.sleep(0.5)
    ui.eval_js("document.getAnimations().forEach(a => a.finish()); return true")
    time.sleep(0.15)


def check(state, size):
    settle()
    r = ui.eval_js(CHECK_JS)
    name = f"{LABEL}-{state}-{size}"
    ui.shot(os.path.join(SHOTS, name + ".png"))
    report[f"{state}@{size}"] = r
    flag = "OK " if r["count"] == 0 else f"{r['count']:3}"
    print(f"{flag} {state} @ {size}", flush=True)
    for kind, what, detail in r["issues"][:8]:
        print(f"      {kind}: {what} {detail}", flush=True)


def esc():
    # Like a real key press: from whatever has focus (the page body when nothing does).
    ui.eval_js('(document.activeElement || document.body).dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true})); return true')
    time.sleep(0.3)


def close_layers():
    for _ in range(3):
        esc()
    ui.eval_js("window.__CN_STORE__.getState().openPanel(null); return true")


SIZES = [tuple(int(n) for n in s.split("x")) for s in SIZE_ARG.split(",")]
if THEME:
    ui.eval_js(f'window.__CN_STORE__.getState().updateSettings({{ theme: "{THEME}" }}); return true')
    time.sleep(0.6)
video = os.path.join(ROOT, "build", "testmedia", "বাংলা নমুনা (Bangla sample).mp4")

for w, h in SIZES:
    size = f"{w}x{h}"
    ui.resize(w, h)
    close_layers()
    flow.go_home()
    check("home", size)

    # Paste a video link (Blender's CC-BY Big Buck Bunny; needs the link programs installed).
    ui.eval_js("""const f = document.querySelector('.linkbar__input');
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
        set.call(f, 'https://www.youtube.com/watch?v=aqz-KE-bpKQ&t=90'); f.dispatchEvent(new Event('input', {bubbles: true}));
        f.form.requestSubmit(); return true""")
    if ui.wait("return !!document.querySelector('.linkcard:not(.linkcard--loading)') || !!document.querySelector('.linksetup')", 90):
        ui.wait("return document.querySelectorAll('.trim__frames .frame.is-loaded').length >= 4", 20)
        check("link-dialog", size)
    close_layers()

    # Settings: every section.
    for section in ["models", "online", "voice", "languages", "captions", "appearance", "processing", "storage", "about"]:
        ui.eval_js(f'window.__CN_STORE__.getState().openPanel("settings", "{section}"); return true')
        time.sleep(0.4)
        check(f"settings-{section}", size)
    close_layers()
    ui.eval_js('window.__CN_STORE__.getState().openPanel("help"); return true')
    check("help", size)
    close_layers()
    ui.eval_js('window.__CN_STORE__.getState().openPanel("about"); return true')
    check("about", size)
    close_layers()

    # Prepare and workspace with real captions.
    ui.eval_js('window.__CN_STORE__.getState().updateSettings({ onlineRecognition: false }); return true')
    ui.queue(video)
    flow.go_home()
    ui.click("Choose video")
    ui.wait('return !!document.querySelector(".prepare .options")', 60)
    check("prepare", size)
    ui.click("Generate captions")
    time.sleep(1.2)
    check("prepare-progress", size)
    ui.wait('return window.__CN_STORE__.getState().view === "workspace"', 600)
    check("workspace", size)

    # A caption selected and focused (typing aids, mic button).
    ui.eval_js('const t = document.querySelector(".cue__text"); t.focus(); return true')
    check("workspace-editing", size)
    ui.eval_js('document.activeElement.blur(); return true')

    ui.click("Shift timing")
    check("shift-timing", size)
    close_layers()
    ui.click("Auto-sync")
    check("auto-sync", size)
    close_layers()
    ui.click("Style")
    ui.wait('return !!document.querySelector(".stylepanel")', 10)
    check("style-panel", size)
    ui.eval_js("window.__CN_STORE__.getState().setStyleOpen(false); return true")
    ui.click("Add language")
    check("add-language", size)
    close_layers()
    ui.click("Export SRT for CapCut")
    check("export-srt", size)
    close_layers()
    ui.click("Export video")
    check("export-video", size)
    close_layers()
    ui.eval_js('document.querySelector("button[aria-label=\\"More caption actions\\"]").dispatchEvent(new PointerEvent("pointerdown", {bubbles: true, button: 0, pointerType: "mouse"})); return true')
    check("more-menu", size)
    close_layers()

errors = ui.eval_js("return window.__AUDIT_ERRORS__")
report["console_errors"] = errors
print("console errors:", len(errors))
for e in errors[:10]:
    print("   ", e[:200])
ui.resize(1360, 860)
json.dump(report, open(os.path.join(ROOT, "build", "bench", f"ui-audit-{LABEL}.json"), "w"), ensure_ascii=False, indent=1)
