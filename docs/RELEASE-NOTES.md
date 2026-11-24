CaptionNest turns the speech in a video into editable captions on your computer and exports an SRT file for CapCut.

**Which file?** Windows 10/11 → `…-Windows-x64-Setup.exe` · Mac with Apple chip → `…-macOS-AppleSilicon.dmg` · Intel Mac → `…-macOS-Intel.dmg` (macOS 13.3+). Read `START-HERE.md` for installation, first-run setup and CapCut import.

**Signing:** these builds are not code-signed or notarized. Windows SmartScreen shows *More info → Run anyway*; on macOS use System Settings → Privacy & Security → *Open Anyway* once.

**Offline and free:** speech models (and optional translation packs) download once on first use from Hugging Face and argos-net.com; after that everything runs offline. No account, API key or subscription.

**Licenses:** CaptionNest is MIT-licensed. It bundles FFmpeg 9.0.2 under the LGPL v2.1+ — the exact source is attached (`ffmpeg-9.0.2.tar.xz`) — plus whisper.cpp (MIT) and other components listed in `THIRD_PARTY_NOTICES.md`. `SHA256SUMS.txt` lists checksums for every file.
