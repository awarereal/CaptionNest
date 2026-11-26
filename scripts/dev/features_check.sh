#!/bin/bash
# Shows which recent features a built or installed CaptionNest contains, by looking for
# names that only exist with each feature (Rust command and setting names are stored as
# plain text in the program file). Short one-off strings make poor markers: an Intel
# build stores "online_credits_deepgram" as instructions, so it isn't searched for.
# usage: scripts/dev/features_check.sh [/path/to/CaptionNest.app]   (default: /Applications)
set -u
APP="${1:-/Applications/CaptionNest.app}"
BIN="$APP/Contents/MacOS/captionnest"
[ -f "$BIN" ] || { echo "not found: $BIN"; exit 1; }
echo "$APP  (built $(stat -f '%Sm' -t '%Y-%m-%d %H:%M' "$BIN"))"
TEXT=$(strings "$BIN")
has() { grep -q -- "$1" <<<"$TEXT" && echo yes || echo NO; }
printf '  %-46s %s\n' "Themes / Appearance (PR 6)" "$(has 'tungsten')"
printf '  %-46s %s\n' "Deepgram timing, engine choice (PR 7)" "$(has 'onlineEngine')"
printf '  %-46s %s\n' "Paste a video link (PR 10)" "$(has 'probe_link')"
printf '  %-46s %s\n' "Video-link downloader setup (PR 10)" "$(has 'link_tools_state')"
COPY=$(/usr/libexec/PlistBuddy -c "Print NSHumanReadableCopyright" "$APP/Contents/Info.plist" 2>/dev/null)
printf '  %-46s %s\n' "Made by Aware (PR 6)" "$(grep -q 'Made by Aware' <<<"$COPY" && echo yes || echo NO)"
NET=$("$APP/Contents/Resources/tools/ffmpeg" -hide_banner -protocols 2>/dev/null | grep -c https)
printf '  %-46s %s\n' "FFmpeg with network (for links, PR 10)" "$([ "$NET" -gt 0 ] && echo yes || echo NO)"
