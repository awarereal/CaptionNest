#!/usr/bin/env bash
# Build a self-contained whisper-cli for macOS.
#
#   scripts/tools/build-whisper-macos.sh arm64|x86_64
#
# arm64: Metal GPU backend (shader library embedded) + Accelerate, CPU fallback with --no-gpu.
# x86_64: CPU + Accelerate (Metal on Intel GPUs is not reliable enough to ship).
# Output: build/tools/<target-triple>/whisper-cli
set -euo pipefail

ARCH="${1:?usage: build-whisper-macos.sh arm64|x86_64}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=versions.env
source "$ROOT/scripts/tools/versions.env"

case "$ARCH" in
  arm64)  TRIPLE=aarch64-apple-darwin; METAL=ON ;;
  x86_64) TRIPLE=x86_64-apple-darwin;  METAL=OFF ;;
  *) echo "unknown arch $ARCH" >&2; exit 2 ;;
esac

"$ROOT/scripts/tools/fetch-sources.sh"

WORK="$ROOT/build/work/whisper-$ARCH"
OUT="$ROOT/build/tools/$TRIPLE"
rm -rf "$WORK"; mkdir -p "$WORK/src" "$OUT"
tar -xzf "$ROOT/build/src/whisper.cpp-$WHISPER_VERSION.tar.gz" -C "$WORK/src" --strip-components=1

GEN=()
command -v ninja >/dev/null 2>&1 && GEN=(-G Ninja)

cmake -S "$WORK/src" -B "$WORK/build" ${GEN[@]+"${GEN[@]}"} \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_OSX_ARCHITECTURES="$ARCH" \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_NATIVE=OFF \
  -DGGML_METAL="$METAL" \
  -DGGML_METAL_EMBED_LIBRARY="$METAL" \
  -DGGML_BLAS=ON \
  -DWHISPER_BUILD_TESTS=OFF \
  -DWHISPER_BUILD_SERVER=OFF \
  -DWHISPER_SDL2=OFF \
  -DWHISPER_CURL=OFF

cmake --build "$WORK/build" --config Release --target whisper-cli -j"$(sysctl -n hw.ncpu)"

cp "$WORK/build/bin/whisper-cli" "$OUT/whisper-cli"
strip -x "$OUT/whisper-cli"
echo "whisper.cpp $WHISPER_VERSION ($WHISPER_URL), Metal=$METAL, target $TRIPLE" > "$OUT/WHISPER_BUILD.txt"

echo "built: $OUT/whisper-cli"
lipo -info "$OUT/whisper-cli"
otool -L "$OUT/whisper-cli"
