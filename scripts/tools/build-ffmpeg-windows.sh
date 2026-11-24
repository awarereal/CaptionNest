#!/usr/bin/env bash
# Build a minimal, LGPL-only, statically linked ffmpeg.exe + ffprobe.exe for Windows x64.
# Run inside an MSYS2 MINGW64 shell with: mingw-w64-x86_64-gcc make nasm pkgconf
# mingw-w64-x86_64-zlib diffutils (CI installs these with msys2/setup-msys2).
#
# Output: build/tools/x86_64-pc-windows-msvc/{ffmpeg.exe,ffprobe.exe,FFMPEG_CONFIGURE.txt}
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=versions.env
source <(tr -d '\r' < "$ROOT/scripts/tools/versions.env")
TRIPLE=x86_64-pc-windows-msvc

"$ROOT/scripts/tools/fetch-sources.sh"

WORK="$ROOT/build/work/ffmpeg-win64"
OUT="$ROOT/build/tools/$TRIPLE"
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"
tar -xJf "$ROOT/build/src/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$WORK" --strip-components=1

CONFIGURE_ARGS=(
  --prefix="$WORK/install"
  --arch=x86_64 --target-os=mingw32
  --enable-static --disable-shared
  --disable-gpl --disable-nonfree
  --disable-autodetect
  --enable-w32threads --enable-mediafoundation --enable-zlib
  --disable-network
  --disable-doc --disable-htmlpages --disable-manpages --disable-podpages --disable-txtpages
  --disable-ffplay
  --disable-debug
  --disable-indevs --disable-outdevs
  --pkg-config-flags=--static
  --extra-ldflags=-static
)

cd "$WORK"
./configure "${CONFIGURE_ARGS[@]}" | tee "$WORK/configure.log" | tail -n 20
grep -q "^License: LGPL version 2.1 or later" "$WORK/configure.log" || {
  echo "Refusing to continue: FFmpeg license is not LGPL 2.1+" >&2; exit 1; }

make -j"$(nproc)" ffmpeg.exe ffprobe.exe
cp ffmpeg.exe ffprobe.exe "$OUT/"
strip "$OUT/ffmpeg.exe" "$OUT/ffprobe.exe"

{
  echo "FFmpeg $FFMPEG_VERSION (LGPL v2.1 or later)"
  echo "Source: $FFMPEG_URL"
  echo "SHA-256: $FFMPEG_SHA256"
  echo "Target: $TRIPLE (MinGW-w64, static)"
  echo "Modifications: none"
  echo "Configure: ./configure ${CONFIGURE_ARGS[*]}" | sed "s#$WORK/install#<prefix>#"
} > "$OUT/FFMPEG_CONFIGURE.txt"

echo "DLL imports (must be Windows system libraries only):"
objdump -p "$OUT/ffmpeg.exe" | grep "DLL Name" | sort -u
