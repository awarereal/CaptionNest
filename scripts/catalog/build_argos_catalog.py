#!/usr/bin/env python3
"""Build CaptionNest's pinned catalog of Argos Translate language packages.

Developer tool, run when updating the catalog (not at app runtime). For each package in
the official Argos index it downloads the .argosmodel once, records its size and SHA-256,
and checks which tokenizer it uses. CaptionNest's Rust translator supports SentencePiece
packages; BPE packages are recorded but marked unsupported.

Usage: python3 scripts/catalog/build_argos_catalog.py [--out src-tauri/catalog/argos.json]
"""
import argparse
import hashlib
import json
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

INDEX_URL = "https://raw.githubusercontent.com/argosopentech/argospm-index/main/index.json"
UA = {"User-Agent": "CaptionNest-catalog-builder"}


def fetch(url, dest):
    h = hashlib.sha256()
    size = 0
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r, open(dest, "wb") as f:
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            h.update(chunk)
            f.write(chunk)
            size += len(chunk)
    return size, h.hexdigest()


def inspect(path):
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        bad = z.testzip()
        if bad:
            raise ValueError(f"corrupt member {bad}")
        tokenizer = "sentencepiece" if any(n.endswith("/sentencepiece.model") for n in names) else (
            "bpe" if any(n.endswith("/bpe.model") for n in names) else "unknown")
        meta_name = next(n for n in names if n.endswith("/metadata.json") and n.count("/") == 1)
        meta = json.loads(z.read(meta_name))
        has_model = any("/model/model.bin" in n for n in names)
        return tokenizer, meta, has_model


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="src-tauri/catalog/argos.json")
    ap.add_argument("--only", default="", help="comma list like en_bn,bn_en (default: all)")
    a = ap.parse_args()

    index = json.load(urllib.request.urlopen(urllib.request.Request(INDEX_URL, headers=UA), timeout=60))
    only = {x for x in a.only.split(",") if x}
    out = []
    for pkg in index:
        key = f"{pkg['from_code']}_{pkg['to_code']}"
        if only and key not in only:
            continue
        url = next((l for l in pkg.get("links", []) if l.startswith("https://")), None)
        if not url:
            print(f"skip {key}: no https link", flush=True)
            continue
        with tempfile.TemporaryDirectory() as td:
            dest = Path(td) / "pkg.argosmodel"
            try:
                size, sha = fetch(url, dest)
                tokenizer, meta, has_model = inspect(dest)
            except Exception as e:  # noqa: BLE001 - report and continue
                print(f"FAIL {key}: {e}", flush=True)
                continue
        entry = {
            "id": key,
            "from": pkg["from_code"],
            "to": pkg["to_code"],
            "fromName": pkg.get("from_name"),
            "toName": pkg.get("to_name"),
            "version": pkg.get("package_version"),
            "url": url,
            "sizeBytes": size,
            "sha256": sha,
            "tokenizer": tokenizer,
            "supported": tokenizer == "sentencepiece" and has_model,
            "targetPrefix": meta.get("target_prefix", ""),
        }
        out.append(entry)
        print(f"ok   {key:8} {size/1e6:7.1f} MB {tokenizer}", flush=True)

    out.sort(key=lambda e: e["id"])
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps({"source": INDEX_URL, "packages": out}, ensure_ascii=False, indent=1) + "\n")
    print(f"wrote {a.out}: {len(out)} packages, {sum(e['supported'] for e in out)} supported")


if __name__ == "__main__":
    sys.exit(main())
