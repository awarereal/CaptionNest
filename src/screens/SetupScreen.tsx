import { ArrowRight, ShieldCheck, Globe, HardDrives } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { ModelCard } from "../components/ModelCard";
import { Button, formatBytes } from "../components/ui";
import { recommendedModelId } from "../lib/models";
import { LogoMark } from "../components/Logo";

const ORDER = ["large-v3-q5_0", "large-v3-turbo-q5_0", "small"];
const order = (rec: string) => [rec, ...ORDER.filter((id) => id !== rec)];

export function SetupScreen() {
  const models = useStore((s) => s.models);
  const settings = useStore((s) => s.settings);
  const info = useStore((s) => s.info);
  const recommended = recommendedModelId(info);
  const update = useStore((s) => s.updateSettings);
  const setView = useStore((s) => s.setView);
  const whisper = [...(models?.whisper ?? [])].sort((a, b) => order(recommended).indexOf(a.id) - order(recommended).indexOf(b.id));
  const installed = whisper.filter((m) => m.installed);
  const ready = installed.length > 0;

  const finish = async () => {
    const pick = installed.find((m) => m.id === settings?.transcribeModelId) ?? installed.find((m) => m.id === recommended) ?? installed[0];
    await update({ setupComplete: true, transcribeModelId: pick?.id ?? null });
    setView("home");
  };

  return (
    <div className="setup">
      <header className="setup__head">
        <LogoMark size={44} />
        <div>
          <h1 className="setup__title display">Set up offline captioning</h1>
          <p className="setup__lede">
            CaptionNest recognizes speech on this computer. Download one speech model once, and captions work without an internet
            connection from then on.
          </p>
        </div>
      </header>

      <section aria-labelledby="models-title">
        <div className="setup__section-head">
          <h2 id="models-title" className="section-label">Choose a speech model</h2>
          <span className="hint">You can add or remove models later in Settings.</span>
        </div>
        <div className="setup__models">
          {whisper.map((m) => (
            <ModelCard
              key={m.id}
              entry={m}
              recommended={m.id === recommended}
              selected={settings?.transcribeModelId === m.id}
              onUse={() => void update({ transcribeModelId: m.id })}
            />
          ))}
        </div>
      </section>

      <section className="setup__facts">
        <div className="fact">
          <Globe size={20} />
          <p>
            <strong>Internet is only used for this download.</strong> Speech models come from Hugging Face (huggingface.co),
            translation language packs from Argos Translate (argos-net.com). Each file is checked against a known fingerprint
            before it's used.
          </p>
        </div>
        <div className="fact">
          <ShieldCheck size={20} />
          <p>
            <strong>Your videos stay on this computer.</strong> Audio, transcripts and captions are processed and saved locally.
            No account, no API key, no subscription.
          </p>
        </div>
        <div className="fact">
          <HardDrives size={20} />
          <p>
            <strong>Storage:</strong> models are kept in CaptionNest's app data folder
            {models?.freeBytes != null && <> · {formatBytes(models.freeBytes)} free on this disk</>}.
          </p>
        </div>
      </section>

      <footer className="setup__foot">
        <Button variant="ghost" onClick={() => void update({ setupComplete: true }).then(() => setView("home"))}>
          Skip for now
        </Button>
        <Button variant="primary" size="lg" trailing={<ArrowRight size={18} weight="bold" />} disabled={!ready} onClick={() => void finish()}>
          {ready ? "Start captioning" : "Download a model to continue"}
        </Button>
      </footer>
    </div>
  );
}
