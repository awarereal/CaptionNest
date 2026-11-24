import { useEffect, useMemo, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Waveform, Translate, TextAa, Cpu, HardDrives, Info, MagnifyingGlass, DownloadSimple, Trash, CheckCircle } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { ModelCard, DownloadStatus } from "../components/ModelCard";
import { Button, Modal, Switch, formatBytes } from "../components/ui";
import { language } from "../lib/languages";
import { recommendedModelId } from "../lib/models";
import type { ArgosEntry } from "../lib/types";

const SECTIONS = [
  { id: "models", label: "Speech models", icon: Waveform },
  { id: "languages", label: "Language packs", icon: Translate },
  { id: "captions", label: "Caption layout", icon: TextAa },
  { id: "processing", label: "Processing", icon: Cpu },
  { id: "storage", label: "Storage", icon: HardDrives },
  { id: "about", label: "About", icon: Info },
] as const;

export function SettingsPanel() {
  const panel = useStore((s) => s.panel);
  const section = useStore((s) => s.settingsSection);
  const openPanel = useStore((s) => s.openPanel);
  const refresh = useStore((s) => s.refreshModels);
  const [active, setActive] = useState<string>("models");

  useEffect(() => {
    if (panel === "settings") {
      setActive(section ?? "models");
      void refresh();
    }
  }, [panel, section, refresh]);

  return (
    <Modal open={panel === "settings"} onOpenChange={(o) => !o && openPanel(null)} title="Settings" size="xl" className="settings">
      <div className="settings__layout">
        <nav className="settings__nav" aria-label="Settings sections">
          {SECTIONS.map(({ id, label, icon: Icon }) => (
            <button key={id} className={`settings__navitem ${active === id ? "is-active" : ""}`} onClick={() => setActive(id)} aria-current={active === id}>
              <Icon size={17} /> {label}
            </button>
          ))}
        </nav>
        <div className="settings__content">
          {active === "models" && <ModelsSection />}
          {active === "languages" && <LanguagesSection />}
          {active === "captions" && <CaptionsSection />}
          {active === "processing" && <ProcessingSection />}
          {active === "storage" && <StorageSection />}
          {active === "about" && <AboutSection />}
        </div>
      </div>
    </Modal>
  );
}

function SectionHead({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <header className="settings__head">
      <h3 className="display">{title}</h3>
      {children && <p className="hint">{children}</p>}
    </header>
  );
}

function ModelsSection() {
  const models = useStore((s) => s.models);
  const settings = useStore((s) => s.settings);
  const info = useStore((s) => s.info);
  const update = useStore((s) => s.updateSettings);
  const rec = recommendedModelId(info);
  return (
    <>
      <SectionHead title="Speech models">
        Models turn speech into text on this computer. Downloaded once from Hugging Face, then used offline.
      </SectionHead>
      <div className="settings__models">
        {(models?.whisper ?? []).map((m) => (
          <ModelCard
            key={m.id}
            entry={m}
            recommended={m.id === rec}
            selected={settings?.transcribeModelId === m.id}
            onUse={() => void update({ transcribeModelId: m.id })}
            compact={false}
          />
        ))}
      </div>
    </>
  );
}

function packTitle(p: ArgosEntry) {
  const from = language(p.from)?.name ?? p.fromName ?? p.from;
  const to = language(p.to)?.name ?? p.toName ?? p.to;
  return { from, to };
}

function LanguagesSection() {
  const models = useStore((s) => s.models);
  const download = useStore((s) => s.download);
  const remove = useStore((s) => s.removeModel);
  const downloads = useStore((s) => s.downloads);
  const [q, setQ] = useState("");
  const packs = useMemo(() => {
    const all = models?.argos ?? [];
    const query = q.trim().toLowerCase();
    const match = (p: ArgosEntry) => {
      if (!query) return true;
      const { from, to } = packTitle(p);
      return `${from} ${to} ${p.from} ${p.to} ${language(p.to)?.native ?? ""} ${language(p.from)?.native ?? ""}`.toLowerCase().includes(query);
    };
    const list = all.filter(match);
    const sort = (a: ArgosEntry, b: ArgosEntry) => Number(b.installed) - Number(a.installed) || packTitle(a).to.localeCompare(packTitle(b).to);
    return {
      fromEnglish: list.filter((p) => p.from === "en").sort(sort),
      intoEnglish: list.filter((p) => p.to === "en").sort((a, b) => Number(b.installed) - Number(a.installed) || packTitle(a).from.localeCompare(packTitle(b).from)),
      other: list.filter((p) => p.from !== "en" && p.to !== "en"),
    };
  }, [models, q]);

  const row = (p: ArgosEntry) => {
    const { from, to } = packTitle(p);
    const d = downloads[p.id];
    return (
      <li key={p.id} className="pack">
        <div className="pack__names">
          <span className="pack__title">
            {from} <span className="pack__arrow">→</span> {to}
          </span>
          <span className="hint mono">{formatBytes(p.sizeBytes)}</span>
        </div>
        <div className="pack__action">
          {p.installed ? (
            <>
              <span className="pack__ready">
                <CheckCircle size={15} weight="fill" /> Installed
              </span>
              <Button size="sm" variant="ghost" icon={<Trash size={14} />} onClick={() => void remove(p.id)} aria-label={`Remove ${from} to ${to}`}>
                Remove
              </Button>
            </>
          ) : d && d.state !== "failed" ? (
            <DownloadStatus id={p.id} total={p.sizeBytes} />
          ) : (
            <>
              {d?.state === "failed" && <DownloadStatus id={p.id} total={p.sizeBytes} />}
              <Button size="sm" variant="secondary" icon={<DownloadSimple size={14} />} onClick={() => void download(p.id)}>
                {d?.state === "failed" ? "Try again" : "Download"}
              </Button>
            </>
          )}
        </div>
      </li>
    );
  };

  return (
    <>
      <SectionHead title="Language packs">
        Offline text translation from Argos Translate (argos-net.com). To get captions in a language, install the pack from English into it. Other
        spoken languages also need their pack into English; CaptionNest translates through English when there's no direct pack.
      </SectionHead>
      <div className="search">
        <MagnifyingGlass size={16} />
        <input className="search__input" placeholder="Search languages" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search language packs" />
      </div>
      <h4 className="section-label">Captions in another language (from English)</h4>
      <ul className="packs">{packs.fromEnglish.map(row)}</ul>
      <h4 className="section-label">Into English (used when translating other languages)</h4>
      <ul className="packs">{packs.intoEnglish.map(row)}</ul>
      {packs.other.length > 0 && (
        <>
          <h4 className="section-label">Direct pairs</h4>
          <ul className="packs">{packs.other.map(row)}</ul>
        </>
      )}
      <p className="hint settings__footnote">
        Packs not listed here use a different text format that CaptionNest doesn't support yet. Quality varies by language; machine translation is a
        draft, so review it before publishing.
      </p>
    </>
  );
}

function CaptionsSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const n = settings?.maxLineChars ?? 42;
  const sample = "Today we're going for a walk by the river, because the weather is lovely.";
  const lines = wrapPreview(sample, n);
  return (
    <>
      <SectionHead title="Caption layout">How new captions are split into lines. Existing captions aren't changed.</SectionHead>
      <div className="formrow">
        <label className="label" htmlFor="maxchars">
          Characters per line: <span className="mono">{n}</span>
        </label>
        <input
          id="maxchars"
          type="range"
          min={28}
          max={60}
          step={1}
          value={n}
          className="slider"
          onChange={(e) => void update({ maxLineChars: Number(e.target.value) })}
        />
        <div className="slider__marks hint">
          <span>Short lines (vertical videos)</span>
          <span>Long lines (wide videos)</span>
        </div>
      </div>
      <div className="layout-preview" aria-hidden>
        {lines.map((l, i) => (
          <span key={i} className="overlay-cue__text caption-text">
            {l}
          </span>
        ))}
      </div>
      <p className="hint">Common choices: 42 for landscape video, around 32 for vertical short-form video. Captions use at most two lines.</p>
    </>
  );
}

function wrapPreview(text: string, max: number): string[] {
  const words = text.split(" ");
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    if ((cur + " " + w).trim().length > max && cur) {
      lines.push(cur);
      cur = w;
    } else cur = (cur + " " + w).trim();
    if (lines.length === 2) break;
  }
  if (lines.length < 2 && cur) lines.push(cur);
  return lines.slice(0, 2);
}

function ProcessingSection() {
  const settings = useStore((s) => s.settings);
  const info = useStore((s) => s.info);
  const update = useStore((s) => s.updateSettings);
  return (
    <>
      <SectionHead title="Processing">Everything runs on this computer.</SectionHead>
      <div className="setting-row">
        <div>
          <label className="setting-row__title" htmlFor="gpu">
            Use graphics acceleration
          </label>
          <p className="hint">
            {info?.gpuBackend === "Metal"
              ? "Uses the Apple Silicon graphics chip (Metal) for much faster speech recognition."
              : info?.gpuBackend === "Vulkan"
                ? "Uses your graphics card through Vulkan when its driver supports it."
                : "This computer uses the processor for speech recognition."}{" "}
            If acceleration fails, CaptionNest automatically finishes on the processor.
          </p>
        </div>
        <Switch id="gpu" label="Use graphics acceleration" checked={settings?.useGpu ?? true} onChange={(v) => void update({ useGpu: v })} />
      </div>
    </>
  );
}

function StorageSection() {
  const models = useStore((s) => s.models);
  return (
    <>
      <SectionHead title="Storage">Downloaded models and language packs. Videos and projects are never copied here.</SectionHead>
      <dl className="kv">
        <div>
          <dt>Used by downloads</dt>
          <dd className="mono">{formatBytes(models?.diskUsageBytes ?? 0)}</dd>
        </div>
        <div>
          <dt>Free on this disk</dt>
          <dd className="mono">{models?.freeBytes != null ? formatBytes(models.freeBytes) : "Unknown"}</dd>
        </div>
        <div>
          <dt>Location</dt>
          <dd className="selectable mono kv__path">{models?.modelsDir}</dd>
        </div>
      </dl>
      <Button variant="secondary" onClick={() => models && void revealItemInDir(models.modelsDir)}>
        Show in {navigator.userAgent.includes("Mac") ? "Finder" : "File Explorer"}
      </Button>
      <p className="hint settings__footnote">To free space, remove models or language packs you don't use from their sections.</p>
    </>
  );
}

function AboutSection() {
  const info = useStore((s) => s.info);
  const openPanel = useStore((s) => s.openPanel);
  return (
    <>
      <SectionHead title="About CaptionNest" />
      <dl className="kv">
        <div>
          <dt>Version</dt>
          <dd className="mono">{info?.version}</dd>
        </div>
        <div>
          <dt>System</dt>
          <dd className="mono">
            {info?.os} · {info?.arch}
          </dd>
        </div>
      </dl>
      <p className="settings__para">
        CaptionNest is free software. Speech recognition uses whisper.cpp with OpenAI Whisper models; translation uses Argos Translate packages run
        through CTranslate2; media handling uses FFmpeg under the LGPL.
      </p>
      <Button variant="secondary" onClick={() => openPanel("about")}>
        Licenses and credits
      </Button>
    </>
  );
}
