#!/usr/bin/env python3
"""Developer check: does running 2 whisper-cli processes in parallel speed things up?
Uses the chunk files left by speed_test.py (build/samples/speed/c*.wav)."""
import glob
import subprocess
import sys
import time

T = "src-tauri/resources/tools/whisper-cli"
model = sys.argv[1] if len(sys.argv) > 1 else "large-v3-q5_0"
extra = ["-ng"] if "--cpu" in sys.argv else []
files = sorted(glob.glob("build/samples/speed/c*.wav"))
base = [T, "-m", f"build/models/ggml-{model}.bin", "-l", "bn", "-oj", "-ml", "1", "-sow", "-sns", "-ac", "768"] + extra


def run(groups):
    procs = []
    t0 = time.time()
    for g in groups:
        args = list(base)
        for f in g:
            args += ["-f", f]
        procs.append(subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    for p in procs:
        p.wait()
    return time.time() - t0


for n in (1, 2, 3):
    groups = [files[i::n] for i in range(n)]
    print(f"{model} {'cpu' if extra else 'gpu'} processes={n}: {run(groups):.1f}s for {len(files)} chunks", flush=True)
