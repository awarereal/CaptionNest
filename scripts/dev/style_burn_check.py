#!/usr/bin/env python3
"""UI check: caption styles and exporting a video with burned-in captions.

Generates Bangla captions, opens the style panel, applies presets (screenshots), then
exports a video through the Export video dialog and checks the result with ffprobe and
frame grabs at caption times.
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402
import ui  # noqa: E402

ROOT = flow.ROOT
FFMPEG = os.path.join(ROOT, "src-tauri", "target", "release", "tools", "ffmpeg")
FFPROBE = os.path.join(ROOT, "src-tauri", "target", "release", "tools", "ffprobe")
OUT_DIR = os.path.join(ROOT, "build", "burn")
os.makedirs(OUT_DIR, exist_ok=True)

ui.wait('return !!window.__CN_STORE__ && !!document.querySelector(".app")', 120)
video = os.path.join(ROOT, "build", "testmedia", "বাংলা নমুনা (Bangla sample).mp4")
r = flow.generate(video, spoken="bn", caption="same")
cues = r["cues"]

# Style panel: open, screenshot, try presets.
ui.click("Style")
ui.wait('return !!document.querySelector(".stylepanel")', 10)
ui.eval_js("window.__CN_STORE__.getState().setStyle(" + json.dumps({"font": "anek"}) + "); return true")
time.sleep(0.3)
# Park the video inside the first caption so the preview shows it.
ui.eval_js(f'const v = document.querySelector("video"); v.currentTime = {(cues[0]["start"] + 1500) / 1000}; return true')
time.sleep(0.8)
for preset in ["Clean", "Highlight", "Bold"]:
    ui.eval_js(f'[...document.querySelectorAll(".preset")].find(b => b.innerText.trim() === "{preset}").click(); return true')
    time.sleep(0.8)
    flow.shot(f"style-{preset.lower()}")
ui.eval_js('[...document.querySelectorAll(".preset")].find(b => b.innerText.trim() === "Highlight").click(); return true')
style = ui.eval_js("return window.__CN_STORE__.getState().style")
ui.click("Done")

# Export video.
out = os.path.join(OUT_DIR, "Bangla sample - captions.mp4")
if os.path.exists(out):
    os.remove(out)
ui.click("Export video")
ui.wait('return !!document.querySelector(".vexp")', 15)
time.sleep(0.6)
flow.shot("export-video-dialog")
ui.queue(out)
t0 = time.time()
ui.click("Export video…")
ui.wait('const b = document.querySelector(".burnprog"); return !!b', 20)
time.sleep(1.0)
flow.shot("export-video-progress")
ui.wait('return !!document.querySelector(".export-done")', 600)
seconds = round(time.time() - t0, 1)
flow.shot("export-video-done")
ui.eval_js('const b = [...document.querySelectorAll("button")].find(x => x.innerText.trim() === "Done"); b && b.click(); return true')

probe = json.loads(subprocess.run([FFPROBE, "-v", "error", "-print_format", "json", "-show_streams", "-show_format", out], capture_output=True, text=True).stdout)
streams = [(s["codec_type"], s["codec_name"], s.get("width"), s.get("height")) for s in probe["streams"]]
src = json.loads(subprocess.run([FFPROBE, "-v", "error", "-print_format", "json", "-show_format", video], capture_output=True, text=True).stdout)
grabs = []
for i, c in enumerate(cues[:3]):
    for label, at in (("inside", (c["start"] + c["end"]) / 2), ("before", c["start"] - 120)):
        if at < 0:
            continue
        png = os.path.join(OUT_DIR, f"cue{i}-{label}.png")
        subprocess.run([FFMPEG, "-v", "error", "-y", "-ss", f"{at / 1000:.3f}", "-i", out, "-frames:v", "1", png], check=True)
        grabs.append(png)
report = {
    "ok": os.path.exists(out) and any(s[0] == "video" for s in streams) and any(s[0] == "audio" for s in streams),
    "seconds": seconds,
    "streams": streams,
    "duration_out": probe["format"]["duration"],
    "duration_src": src["format"]["duration"],
    "style": style,
    "frames": grabs,
}
print(json.dumps(report, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["export_video_burned_in"] = report
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
