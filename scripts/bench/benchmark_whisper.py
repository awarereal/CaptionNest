#!/usr/bin/env python3
"""Benchmark Whisper models on real Bangla speech (FLEURS bn_in dev split).

Developer tool only - not shipped. Measures, per model:
  * transcription CER / WER against FLEURS normalized transcripts
  * whether output uses Bengali script (small models often answer in Devanagari)
  * bn->en translation chrF against the parallel FLEURS English sentence
  * real-time factor (processing seconds / audio seconds)

Usage:
  python3 scripts/bench/benchmark_whisper.py --whisper build/tools/aarch64-apple-darwin/whisper-cli \
      --models build/models --fleurs build/fleurs --count 20 [--cpu]
"""
import argparse
import csv
import json
import re
import subprocess
import sys
import time
import unicodedata

from collections import Counter
from pathlib import Path

PUNCT = re.compile(r"[।॥.,!?;:\"'()\[\]{}\-–—…।॥‘’“”]")


def norm(text: str) -> str:
    text = unicodedata.normalize("NFC", text).lower()
    text = PUNCT.sub(" ", text)
    return " ".join(text.split())


def edit_distance(a, b) -> int:
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def chrf(hyp: str, ref: str, n: int = 6, beta: float = 2.0) -> float:
    hyp, ref = hyp.replace(" ", ""), ref.replace(" ", "")
    precs, recs = [], []
    for k in range(1, n + 1):
        h = Counter(hyp[i:i + k] for i in range(len(hyp) - k + 1))
        r = Counter(ref[i:i + k] for i in range(len(ref) - k + 1))
        overlap = sum((h & r).values())
        if sum(h.values()):
            precs.append(overlap / sum(h.values()))
        if sum(r.values()):
            recs.append(overlap / sum(r.values()))
    p = sum(precs) / len(precs) if precs else 0.0
    r = sum(recs) / len(recs) if recs else 0.0
    if p + r == 0:
        return 0.0
    return 100 * (1 + beta**2) * p * r / (beta**2 * p + r)


def to_bengali_script(text: str) -> str:
    """Map stray Devanagari letters to the parallel Bengali code points (same as the app)."""
    out = []
    for c in text:
        o = ord(c)
        if 0x0900 <= o <= 0x097F and o not in (0x0964, 0x0965):
            b = chr(o + 0x80)
            if c == "\u0935":  # va -> ba
                b = "\u09AC"
            out.append(b if unicodedata.name(b, "") else c)
        else:
            out.append(c)
    return "".join(out)


def bengali_ratio(text: str) -> float:
    letters = [c for c in text if c.isalpha()]
    if not letters:
        return 0.0
    return sum(1 for c in letters if "ঀ" <= c <= "৿") / len(letters)


FFPROBE = None


def wav_seconds(path: Path) -> float:
    out = subprocess.run(
        [FFPROBE, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout.strip())


def run(whisper, model, wav, translate, cpu):
    args = [whisper, "-m", str(model), "-f", str(wav), "-l", "bn", "-sns", "-nt", "-np"]
    if translate:
        args.append("-tr")
    if cpu:
        args.append("-ng")
    t0 = time.perf_counter()
    out = subprocess.run(args, capture_output=True)
    elapsed = time.perf_counter() - t0
    text = out.stdout.decode("utf-8", errors="replace").strip()
    return text, elapsed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--whisper", required=True)
    ap.add_argument("--models", required=True)
    ap.add_argument("--fleurs", required=True)
    ap.add_argument("--count", type=int, default=20)
    ap.add_argument("--names", default="small,medium-q5_0,large-v3-turbo-q5_0,large-v3-q5_0")
    ap.add_argument("--translate-names", default="medium-q5_0,large-v3-q5_0")
    ap.add_argument("--cpu", action="store_true")
    ap.add_argument("--out", default="build/bench/results.json")
    a = ap.parse_args()

    global FFPROBE
    FFPROBE = str(Path(a.whisper).with_name("ffprobe"))
    fleurs = Path(a.fleurs)
    english = {}
    en_tsv = fleurs / "en_dev.tsv"
    if en_tsv.exists():
        for row in csv.reader(en_tsv.open(encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE):
            english.setdefault(row[0], row[2])

    seen, items = set(), []
    for row in csv.reader((fleurs / "bn_dev.tsv").open(encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE):
        sid, file, normalized = row[0], row[1], row[3]
        if sid in seen:
            continue
        wav = fleurs / "dev" / file
        if wav.exists():
            seen.add(sid)
            items.append((sid, wav, normalized, english.get(sid)))
        if len(items) >= a.count:
            break

    audio_total = sum(wav_seconds(w) for _, w, _, _ in items)
    results = {"utterances": len(items), "audio_seconds": round(audio_total, 1), "cpu_only": a.cpu, "models": {}}
    print(f"{len(items)} utterances, {audio_total:.0f}s audio, cpu_only={a.cpu}", flush=True)

    for name in a.names.split(","):
        model = Path(a.models) / f"ggml-{name}.bin"
        if not model.exists():
            print(f"skip {name}: missing", flush=True)
            continue
        run(a.whisper, model, items[0][1], False, a.cpu)  # warm-up (shader compile, file cache)
        char_err = char_tot = word_err = word_tot = 0
        fixed_char_err = fixed_word_err = 0
        all_hyps = []
        seconds = 0.0
        scripts = []
        samples = []
        for sid, wav, ref, _ in items:
            hyp, el = run(a.whisper, model, wav, False, a.cpu)
            seconds += el
            h, r = norm(hyp), norm(ref)
            char_err += edit_distance(h.replace(" ", ""), r.replace(" ", ""))
            char_tot += len(r.replace(" ", ""))
            word_err += edit_distance(h.split(), r.split())
            hf = norm(to_bengali_script(hyp))
            fixed_char_err += edit_distance(hf.replace(" ", ""), r.replace(" ", ""))
            fixed_word_err += edit_distance(hf.split(), r.split())
            all_hyps.append(hyp)
            word_tot += len(r.split())
            scripts.append(bengali_ratio(hyp))
            if len(samples) < 3:
                samples.append({"ref": ref, "hyp": hyp})
        entry = {
            "cer": round(100 * char_err / max(char_tot, 1), 1),
            "wer": round(100 * word_err / max(word_tot, 1), 1),
            "cer_script_fixed": round(100 * fixed_char_err / max(char_tot, 1), 1),
            "wer_script_fixed": round(100 * fixed_word_err / max(word_tot, 1), 1),
            "bengali_script_ratio": round(sum(scripts) / len(scripts), 2),
            "hyps": all_hyps,
            "rtf": round(seconds / audio_total, 3),
            "samples": samples,
        }
        if name in a.translate_names.split(",") and english:
            scores, tsec, tsamples = [], 0.0, []
            for sid, wav, _, en in items:
                if not en:
                    continue
                hyp, el = run(a.whisper, model, wav, True, a.cpu)
                tsec += el
                scores.append(chrf(hyp.lower(), en.lower()))
                if len(tsamples) < 3:
                    tsamples.append({"ref": en, "hyp": hyp})
            entry["translate_chrf"] = round(sum(scores) / max(len(scores), 1), 1)
            entry["translate_rtf"] = round(tsec / audio_total, 3)
            entry["translate_samples"] = tsamples
        results["models"][name] = entry
        print(name, json.dumps({k: v for k, v in entry.items() if "samples" not in k and k != "hyps"}), flush=True)

    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    sys.exit(main())
