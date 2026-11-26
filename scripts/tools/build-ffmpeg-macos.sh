#!/usr/bin/env bash
# Build a minimal, LGPL-only, statically linked ffmpeg + ffprobe for macOS.
#
#   scripts/tools/build-ffmpeg-macos.sh arm64|x86_64
#
# - No GPL or nonfree components, no external libraries except system frameworks
#   (VideoToolbox, AudioToolbox, Security, zlib), so the only source to offer is FFmpeg itself.
# - Network protocols use macOS's own TLS (Secure Transport): "Paste a video link" has
#   yt-dlp hand ffmpeg the stream address so only the chosen part is fetched. Nothing
#   else in CaptionNest gives ffmpeg a network address.
# - Output: build/tools/<target-triple>/{ffmpeg,ffprobe} plus FFMPEG_CONFIGURE.txt
set -euo pipefail

ARCH="${1:?usage: build-ffmpeg-macos.sh arm64|x86_64}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=versions.env
source "$ROOT/scripts/tools/versions.env"

case "$ARCH" in
  arm64)  TRIPLE=aarch64-apple-darwin; FF_ARCH=aarch64 ;;
  x86_64) TRIPLE=x86_64-apple-darwin;  FF_ARCH=x86_64 ;;
  *) echo "unknown arch $ARCH" >&2; exit 2 ;;
esac

"$ROOT/scripts/tools/fetch-sources.sh"

WORK="$ROOT/build/work/ffmpeg-$ARCH"
OUT="$ROOT/build/tools/$TRIPLE"
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"
tar -xJf "$ROOT/build/src/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$WORK" --strip-components=1

CROSS=()
if [[ "$(uname -m)" != "$ARCH" ]]; then
  CROSS=(--enable-cross-compile)
fi
ASM=()
if [[ "$ARCH" == "x86_64" ]] && ! command -v nasm >/dev/null 2>&1; then
  echo "nasm not found: building x86_64 without hand-written assembly (slower)." >&2
  ASM=(--disable-x86asm)
fi

MINOS="-mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET"
CONFIGURE_ARGS=(
  --prefix="$WORK/install"
  --arch="$FF_ARCH" --target-os=darwin
  --cc="clang -arch $ARCH" --cxx="clang++ -arch $ARCH"
  --extra-cflags="$MINOS" --extra-ldflags="$MINOS"
  --enable-static --disable-shared
  --disable-gpl --disable-nonfree
  --disable-autodetect
  --enable-videotoolbox --enable-audiotoolbox --enable-zlib
  --enable-network --enable-securetransport
  --disable-doc --disable-htmlpages --disable-manpages --disable-podpages --disable-txtpages
  --disable-ffplay
  --disable-debug
  --disable-indevs --disable-outdevs
  --pkg-config-flags=--static
)

cd "$WORK"
./configure "${CONFIGURE_ARGS[@]}" ${CROSS[@]+"${CROSS[@]}"} ${ASM[@]+"${ASM[@]}"} | tee "$WORK/configure.log" | tail -n 20
grep -q "^License: LGPL version 2.1 or later" "$WORK/configure.log" || {
  echo "Refusing to continue: FFmpeg license is not LGPL 2.1+" >&2; exit 1; }

make -j"$(sysctl -n hw.ncpu)" ffmpeg ffprobe
cp ffmpeg ffprobe "$OUT/"
strip -x "$OUT/ffmpeg" "$OUT/ffprobe"

{
  echo "FFmpeg $FFMPEG_VERSION (LGPL v2.1 or later)"
  echo "Source: $FFMPEG_URL"
  echo "SHA-256: $FFMPEG_SHA256"
  echo "Target: $TRIPLE, macOS $MACOS_DEPLOYMENT_TARGET+"
  echo "Modifications: none"
  echo "Configure: ./configure ${CONFIGURE_ARGS[*]} ${CROSS[*]+"${CROSS[*]}"} ${ASM[*]+"${ASM[*]}"}" | sed "s#$WORK/install#<prefix>#"
} > "$OUT/FFMPEG_CONFIGURE.txt"

echo "built: $OUT/ffmpeg $OUT/ffprobe"
lipo -info "$OUT/ffmpeg"
otool -L "$OUT/ffmpeg"
