#!/usr/bin/env python3
"""Offline evidence: while the app generates and translates captions, sample every
network socket held by the app and its child tools (ffmpeg, whisper-cli, whisper-vad)
with lsof. Any connection that isn't to this computer is reported."""
import json
import os
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(__file__))
import flow  # noqa: E402

ROOT = flow.ROOT
app_pid = subprocess.run(["pgrep", "-f", "target/release/captionnest"], capture_output=True, text=True).stdout.split()[0]
seen, stop = set(), threading.Event()


def pids():
    out = subprocess.run(["pgrep", "-P", app_pid], capture_output=True, text=True).stdout.split()
    return [app_pid] + out


def sample():
    while not stop.is_set():
        for line in subprocess.run(["lsof", "-nP", "-a", "-i", "-p", ",".join(pids())], capture_output=True, text=True).stdout.splitlines()[1:]:
            seen.add(" ".join(line.split()[0:1] + line.split()[7:]))
        time.sleep(0.3)


t = threading.Thread(target=sample)
t.start()
en = flow.generate(os.path.join(ROOT, "build", "testmedia", "টেস্ট ভিডিও - English sample.mp4"), spoken="en", caption="bn")
bn = flow.generate(os.path.join(ROOT, "build", "testmedia", "বাংলা নমুনা (Bangla sample).mp4"), spoken="auto", caption="en")
stop.set()
t.join()
remote = sorted(s for s in seen if "->" in s and not any(h in s.split("->")[1] for h in ("127.0.0.1", "[::1]", "localhost")))
listen_or_local = sorted(s for s in seen if s not in remote)
res = {
    "ok": not remote and en.get("language") == "bn" and bn.get("language") == "en",
    "english_to_bangla_tracks": en.get("tracks"),
    "bangla_to_english_tracks": bn.get("tracks"),
    "remote_connections": remote,
    "local_sockets": listen_or_local,
}
print(json.dumps(res, ensure_ascii=False, indent=1))
path = os.path.join(ROOT, "build", "bench", "verify.json")
prev = json.load(open(path)) if os.path.exists(path) else {}
prev["no_network_during_processing"] = res
json.dump(prev, open(path, "w"), ensure_ascii=False, indent=1)
