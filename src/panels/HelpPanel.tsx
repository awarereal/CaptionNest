import { openUrl } from "@tauri-apps/plugin-opener";
import { Desktop, Globe, DeviceMobile, ArrowSquareOut } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { Modal } from "../components/ui";

const SOURCE = "https://www.capcut.com/help/how-to-import-subtitles";

export function HelpPanel() {
  const panel = useStore((s) => s.panel);
  const openPanel = useStore((s) => s.openPanel);
  return (
    <Modal
      open={panel === "help"}
      onOpenChange={(o) => !o && openPanel(null)}
      title="Use your captions in CapCut"
      description="Import the original video and the exported .srt file. Each caption becomes its own clip you can retime, restyle and edit in CapCut."
      size="lg"
    >
      <div className="help">
        <section className="help__card">
          <h3 className="help__title">
            <Desktop size={20} /> CapCut Desktop (Windows and Mac)
          </h3>
          <ol className="help__steps">
            <li>Start a new project and import your original video, then drag it onto the timeline.</li>
            <li>
              Open the <strong>Captions</strong> tab and choose <strong>Add captions</strong>.
            </li>
            <li>
              Choose <strong>Import file</strong> and select the .srt file from CaptionNest. (Older CapCut versions call this “Import captions” or “Local
              captions”.)
            </li>
            <li>The captions appear on their own track, lined up with the video. Click any caption to change its text, timing or style.</li>
          </ol>
        </section>

        <section className="help__card">
          <h3 className="help__title">
            <Globe size={20} /> CapCut on the web (capcut.com)
          </h3>
          <ol className="help__steps">
            <li>Open the editor and upload your original video.</li>
            <li>
              Select <strong>Captions</strong> in the left toolbar, then <strong>Upload caption file</strong>.
            </li>
            <li>Choose the .srt file. CapCut Web accepts SRT (not TXT) caption files.</li>
          </ol>
        </section>

        <section className="help__card help__card--muted">
          <h3 className="help__title">
            <DeviceMobile size={20} /> CapCut mobile app
          </h3>
          <p>
            CapCut's help center states that the iOS and Android apps can't import .srt files. Import the captions with CapCut Desktop or Web; CapCut
            says captions imported there can still be edited on mobile afterwards.
          </p>
        </section>

        <div className="callout">
          <div>
            <strong>Tips.</strong> Keep using the original video file; the captions are timed to it. Fonts, colors, animations and position are set in
            CapCut, not in the .srt file. If CapCut shows strange characters, make sure you picked the file exported by CaptionNest (it's always UTF-8).
          </div>
        </div>

        <p className="hint help__source">
          Based on CapCut's official guide, checked November 2026. CapCut changes its menus from time to time.{" "}
          <button className="btn btn--quiet" onClick={() => void openUrl(SOURCE)}>
            Open the CapCut guide <ArrowSquareOut size={13} />
          </button>
        </p>
      </div>
    </Modal>
  );
}
