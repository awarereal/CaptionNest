import { useStore } from "../state/store";
import { Modal } from "../components/ui";
import { LogoMark } from "../components/Logo";
import notices from "../../THIRD_PARTY_NOTICES.md?raw";

export function AboutPanel() {
  const panel = useStore((s) => s.panel);
  const openPanel = useStore((s) => s.openPanel);
  const info = useStore((s) => s.info);
  return (
    <Modal open={panel === "about"} onOpenChange={(o) => !o && openPanel(null)} title="Licenses and credits" size="lg">
      <div className="about">
        <div className="about__brand">
          <LogoMark size={48} />
          <div>
            <p className="display about__name">CaptionNest {info?.version}</p>
            <p className="hint">MIT License. Free to use and share. No account, no subscription, no data collection.</p>
          </div>
        </div>
        <pre className="about__notices selectable">{notices}</pre>
      </div>
    </Modal>
  );
}
