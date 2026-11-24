# Building CaptionNest

CaptionNest is a Tauri 2 app: a React + TypeScript interface (`src/`) and a Rust backend
(`src-tauri/`). It bundles command-line tools that are built from pinned, hash-checked
sources:

| Tool | Version | Built by |
|---|---|---|
| FFmpeg (`ffmpeg`, `ffprobe`) — LGPL-only, no external codec libraries | 9.0.2 | `scripts/tools/build-ffmpeg-*.sh` |
| whisper.cpp (`whisper-cli`, `whisper-vad`) | v1.9.6 | `scripts/tools/build-whisper-*` |
| Silero VAD model (885 KB, bundled) | v6.2.0 | downloaded by `fetch-sources.sh` |

Versions, URLs and SHA-256 hashes live in `scripts/tools/versions.env`.
Speech and translation models are **not** bundled; the app downloads them on request
from URLs pinned (with SHA-256) in `src-tauri/catalog/*.json`.

The easiest way to get all installers is GitHub Actions (`.github/workflows/build.yml`):
every push builds macOS Apple Silicon, macOS Intel and Windows x64, runs the tests,
installs each package and runs a speech smoke test on it. Pushing a tag such as `v0.1.0`
also creates a **draft** release with the installers, the FFmpeg source tarball, the
notices and `SHA256SUMS.txt`.

---

## macOS (Apple Silicon or Intel)

Requirements: Xcode Command Line Tools, Homebrew, Node.js 22+, Rust (rustup recommended).

```sh
brew install cmake ninja nasm          # nasm only matters for Intel FFmpeg builds
npm ci

ARCH=$(uname -m)                       # arm64 or x86_64
TRIPLE=$([ "$ARCH" = arm64 ] && echo aarch64-apple-darwin || echo x86_64-apple-darwin)
scripts/tools/build-ffmpeg-macos.sh "$ARCH"    # ~5 min
scripts/tools/build-whisper-macos.sh "$ARCH"   # ~3 min
scripts/tools/stage-tools.sh "$TRIPLE"         # copies + ad-hoc signs into src-tauri/resources

npm test && (cd src-tauri && cargo test --lib)
npx tauri build --target "$TRIPLE"             # first build also compiles CTranslate2 (~10 min)
```

Output: `src-tauri/target/<triple>/release/bundle/dmg/CaptionNest_<version>_<arch>.dmg`.

Without an Apple Developer ID the app is **ad-hoc signed and not notarized**; recipients
use System Settings → Privacy & Security → *Open Anyway* once. To sign and notarize,
set `bundle.macOS.signingIdentity` and the `APPLE_*` notarization variables described in
the Tauri docs (<https://v2.tauri.app/distribute/sign/macos/>).

## Windows x64

Requirements: Visual Studio 2022 Build Tools (C++ workload), CMake, Node.js 22+, Rust,
MSYS2 (for FFmpeg), optionally the Vulkan SDK (GPU acceleration).

```powershell
# In an MSYS2 MINGW64 shell (packages: mingw-w64-x86_64-gcc make nasm pkgconf mingw-w64-x86_64-zlib diffutils):
scripts/tools/build-ffmpeg-windows.sh

# In PowerShell:
scripts/tools/build-whisper-windows.ps1      # add -NoVulkan for a CPU-only build
scripts/tools/stage-tools.ps1
$env:RUSTFLAGS = "-C target-feature=+crt-static"   # no Visual C++ Redistributable needed
npm ci; npm test
cd src-tauri; cargo test --lib; cd ..
npx tauri build
```

Output: `src-tauri/target/release/bundle/nsis/CaptionNest_<version>_x64-setup.exe`
(per-user install, no administrator rights; WebView2 bootstrapper embedded).
The installer is unsigned unless you add a code-signing certificate
(<https://v2.tauri.app/distribute/sign/windows/>), so SmartScreen shows a warning.

## Development

```sh
npm run tauri dev
```

Set `CAPTIONNEST_HOME=/some/folder` to keep models, settings and caches in one folder
(handy for clean-state testing).

### Automated UI testing (developer builds only)

`--features automation` compiles a local bridge (`src-tauri/src/automation.rs`) that lets
scripts drive the real interface and take snapshots of it. It is never part of release
builds.

```sh
CAPTIONNEST_AUTOMATION_PORT=7788 npm run tauri dev -- --features automation
python3 scripts/dev/flow.py generate "build/testmedia/<video>.mp4" --spoken bn --caption en --shots demo
```

`scripts/dev/make-test-media.sh` and `scripts/dev/make_long_video.py` create test videos
with known speech; `scripts/bench/` holds the accuracy benchmarks used to choose models
and decoding settings (see docs/VERIFICATION.md).

### Updating pinned catalogs

- Argos language packs: `python3 scripts/catalog/build_argos_catalog.py` (downloads each
  package once to record size, SHA-256 and tokenizer).
- Whisper models: edit `src-tauri/catalog/whisper.json` with values from the Hugging Face
  API (`https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main`).
- License list: `node scripts/licenses/collect.mjs`.

## FFmpeg source availability (LGPL)

FFmpeg is used unmodified, built without GPL or non-free parts, and runs as a separate
program. Each release ships the exact source tarball (`ffmpeg-9.0.2.tar.xz`) next to the
installers, and the configure line is in the app at `licenses/FFMPEG_BUILD.txt`.
