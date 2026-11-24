# CaptionNest — Start Here

CaptionNest turns the speech in a video into captions you can review and edit, then saves
them as an **SRT file for CapCut**. Everything runs on your own computer: no account, no
subscription, no uploads.

---

## 1. Download the right file

| Your computer | Download |
|---|---|
| Windows 10 or 11 (64-bit) | `CaptionNest-…-Windows-x64-Setup.exe` |
| Mac with an Apple chip (M1, M2, M3, M4…) | `CaptionNest-…-macOS-AppleSilicon.dmg` |
| Mac with an Intel processor | `CaptionNest-…-macOS-Intel.dmg` |

Not sure which Mac you have? Open the Apple menu  → **About This Mac**. If it says
**Chip: Apple M…**, use AppleSilicon. If it says **Processor: Intel**, use Intel.
macOS 13.3 (Ventura) or later is required.

---

## 2. Install

### Windows
1. Double-click the `Setup.exe` file.
2. Windows may show **"Windows protected your PC"**. This appears because the app isn't
   signed with a paid certificate. Click **More info**, then **Run anyway**.
3. The installer sets CaptionNest up for your user account (no administrator password
   needed) and adds it to the Start menu.
   If your PC is missing Microsoft Edge WebView2, the installer fetches it automatically
   (this needs an internet connection once).

### Mac
1. Double-click the `.dmg` file and drag **CaptionNest** into **Applications**.
2. Open CaptionNest from Applications. The first time, macOS says it can't verify the
   developer, because the app isn't notarized by Apple. Click **Done** (or **OK**).
3. Open **System Settings → Privacy & Security**, scroll down to **Security**, and click
   **Open Anyway** next to CaptionNest. Confirm with your password or Touch ID.
   You only need to do this once.

---

## 3. First start: download a speech model

CaptionNest needs a speech recognition model, downloaded once. The setup screen shows
each model's size and what it's good at; the one marked **Recommended** suits your
computer:

| Model | Download | Good for |
|---|---|---|
| **Accurate** | 1.1 GB | Best accuracy, including Bangla. Also makes English captions from other languages. Fastest on Apple-chip Macs. |
| **Fast** | 574 MB | About four times faster, nearly as accurate. Recommended on Windows and Intel Macs. |
| **Light** | 488 MB | Older computers, clear English only. |

- Downloads come from Hugging Face (huggingface.co). Optional translation language packs
  come from Argos Translate (argos-net.com). Every file is checked before it's used.
- A paused or interrupted download continues where it stopped.
- After this, CaptionNest works **without internet**.

---

## 4. Make captions

1. Click **Choose video**, or drag a video onto the window. MP4, MOV, MKV, WebM and most
   audio files work. Your original file is never changed.
2. **Spoken language:** pick the language people speak. *Detect automatically* works for
   most videos; for Bangla, choosing **Bangla** gives the most reliable result.
3. **Caption language:** *Same as speech*, **English** (translated from the speech), or
   another language if you've installed its language pack.
4. Click **Generate captions**. You'll see each step and its progress; you can **Stop**
   at any time. Long videos are fine — roughly, a 10-minute video takes a few minutes on
   an Apple-chip Mac and longer on computers without a supported graphics chip.

---

## 5. Review and edit

- Click any caption to jump the video there. The caption on screen is highlighted.
- Edit the words directly. Change start or end times by typing, or select a time and use
  the ↑ / ↓ keys (0.1 s; hold Shift for 1 s).
- Hover a caption for **Play**, **Split**, **Merge with next** and **Delete**.
  Press Ctrl+Enter (⌘+Enter on Mac) while typing to split at the cursor.
- **Add caption** inserts one at the current position. **Shift timing** moves every
  caption earlier or later.
- **Undo / Redo**: Ctrl+Z / Ctrl+Shift+Z (⌘ on Mac).
- If some speech couldn't be recognized, CaptionNest keeps it as an **empty caption**
  so nothing is missed: listen and type the words, or delete it.
- **Save** keeps your work as a `.captionnest` project. If you move the video later,
  CaptionNest asks you to locate it.
- **Add language** creates a translated caption track; your original captions stay as
  they are.

Speech recognition is very good but not perfect, especially with background music,
several people talking at once, or strong accents. Bangla captions are a strong first
draft that needs a careful read-through.

---

## 6. Import into CapCut

Click **Export SRT for CapCut**. CaptionNest checks the captions (empty text, timing,
overlaps) and saves a UTF-8 `.srt` file.

**CapCut Desktop (Windows / Mac)**
1. Start a project and import your **original video**; drag it onto the timeline.
2. Open the **Captions** tab → **Add captions** → **Import file**, and choose the `.srt`.
   (Some CapCut versions call this *Import captions* or *Local captions*.)
3. Each caption becomes its own clip on a caption track, already timed to the video.
   Change the text, timing, font, style and position in CapCut.

**CapCut on the web (capcut.com)**
1. Upload your original video in the editor.
2. Choose **Captions** in the left toolbar → **Upload caption file** → select the `.srt`.

**CapCut mobile app:** CapCut's help center states that the iOS and Android apps can't
import `.srt` files. Import on CapCut Desktop or Web instead.

The SRT file holds the words and their timing. Fonts, colors, animations and video edits
are done in CapCut.

---

## Help

| Problem | What to do |
|---|---|
| "No speech found" | Pick the spoken language instead of automatic detection, or choose another sound track if the video has several. |
| The video doesn't play in the preview | Click **Make a preview copy**. Captions are still made from the original. |
| It's slow | In Settings → Speech models, use **Fast**. Close other heavy apps. |
| "Not enough disk space" | Free some space; a speech model needs up to 1.1 GB, and processing needs about 2 MB per minute of video. |
| Captions look shifted | Use **Shift timing** with a value like `+0.5` or `-0.25` seconds. |

Models are kept in CaptionNest's app data folder (Settings → Storage shows where, and
lets you remove models). Removing the app: on Windows use *Settings → Apps*; on Mac drag
CaptionNest from Applications to the Trash.
