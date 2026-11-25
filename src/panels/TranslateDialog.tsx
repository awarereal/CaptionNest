import { useEffect, useMemo, useState } from "react";
import { Translate, DownloadSimple, Info } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { api } from "../lib/api";
import { language, languageName } from "../lib/languages";
import { MODEL_INFO } from "../lib/models";
import type { PlanInfo } from "../lib/types";
import { LanguagePicker, type PickerItem } from "../components/LanguagePicker";
import { Button, Modal, formatBytes } from "../components/ui";
import { DownloadStatus } from "../components/ModelCard";

export function TranslateDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const tracks = useStore((s) => s.history.present);
  const models = useStore((s) => s.models);
  const downloads = useStore((s) => s.downloads);
  const download = useStore((s) => s.download);
  const startTranslate = useStore((s) => s.startTranslate);
  const videoMissing = useStore((s) => s.videoMissing);
  const original = tracks.find((t) => t.kind === "original") ?? tracks[0];
  const [target, setTarget] = useState<string>("");
  const [plan, setPlan] = useState<PlanInfo | null>(null);

  const existing = new Set(tracks.map((t) => t.language));
  const translator = models?.whisper.find((m) => m.installed && MODEL_INFO[m.id]?.translates);
  const accurate = models?.whisper.find((m) => m.id === "large-v3-q5_0");

  const items: PickerItem[] = useMemo(() => {
    const out: PickerItem[] = [];
    if (original && original.language !== "en" && !existing.has("en")) {
      out.push({ value: "en", label: "English", group: "Captions", meta: translator ? "From the speech" : "Needs the Accurate model" });
    }
    for (const p of models?.argos ?? []) {
      if (p.from !== "en" || existing.has(p.to) || p.to === original?.language) continue;
      const l = language(p.to);
      out.push({
        value: p.to,
        label: l?.name ?? p.toName ?? p.to,
        native: l?.native,
        group: p.installed ? "Ready offline" : "Download to use",
        meta: p.installed ? undefined : formatBytes(p.sizeBytes),
      });
    }
    return out.sort((a, b) => (a.group === b.group ? a.label.localeCompare(b.label) : 0));
  }, [models, tracks, translator]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (open && !target && items[0]) setTarget(items[0].value);
  }, [open, items, target]);

  useEffect(() => {
    let alive = true;
    if (!original || !target) return;
    api.planTranslation(original.language, target).then((p) => alive && setPlan(p));
    return () => {
      alive = false;
    };
  }, [original, target, models]);

  if (!original) return null;
  const missing = (plan?.missingPackages ?? []).map((id) => models?.argos.find((p) => p.id === id)).filter(Boolean);
  const needsModel = (plan?.needsTranslationModel ?? false) && !translator;
  const needsVideo = plan?.plan?.kind === "whisperEnglish" || (plan?.plan?.kind === "text" && plan.plan.fromWhisperEnglish);
  const blocked = !target || !plan?.plan || missing.length > 0 || needsModel || (needsVideo && videoMissing);

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title="Add a caption language"
      description={`Creates a new track from the ${languageName(original.language)} captions. Your original captions don't change.`}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={<Translate size={16} />}
            disabled={blocked}
            onClick={() => {
              void startTranslate(target);
              onOpenChange(false);
            }}
          >
            Translate
          </Button>
        </>
      }
    >
      <label className="label" htmlFor="tr-target">
        Translate into
      </label>
      {items.length === 0 ? (
        <p className="hint">Every available language is already in this project.</p>
      ) : (
        <LanguagePicker id="tr-target" label="Translate into" value={target} items={items} onChange={setTarget} />
      )}

      {plan?.plan?.kind === "text" && (
        <p className="formrow__note">
          <Info size={14} />
          {plan.plan.fromWhisperEnglish
            ? "Translated from an English version of the speech, so it needs the video and the Accurate model."
            : plan.plan.packages.length > 1
              ? "Goes through English, so some wording may drift. Timing matches the original captions exactly."
              : "Timing matches the original captions exactly."}
        </p>
      )}
      {plan?.plan?.kind === "whisperEnglish" && (
        <p className="formrow__note">
          <Info size={14} /> English is translated from the speech itself, which needs the original video. Timing follows the speech.
        </p>
      )}
      {needsVideo && videoMissing && <p className="dl__error">Locate the video first; this translation listens to the speech again.</p>}

      {needsModel && accurate && (
        <div className="callout callout--gold">
          <DownloadSimple size={18} />
          <div className="callout__body">
            <p>
              <strong>Needs the Accurate model</strong> ({formatBytes(accurate.sizeBytes)}).
            </p>
            {downloads[accurate.id] ? (
              <DownloadStatus id={accurate.id} total={accurate.sizeBytes} partial={accurate.partialBytes} />
            ) : (
              <Button size="sm" variant="secondary" onClick={() => void download(accurate.id)}>
                Download now
              </Button>
            )}
          </div>
        </div>
      )}
      {missing.length > 0 && (
        <div className="callout callout--gold">
          <DownloadSimple size={18} />
          <div className="callout__body">
            <p>
              <strong>Language {missing.length === 1 ? "pack" : "packs"} needed</strong> (
              {formatBytes(missing.reduce((a, p) => a + (p?.sizeBytes ?? 0), 0))}).
            </p>
            {missing.map((p) => (downloads[p!.id] ? <DownloadStatus key={p!.id} id={p!.id} total={p!.sizeBytes} /> : null))}
            {missing.some((p) => !downloads[p!.id]) && (
              <Button size="sm" variant="secondary" onClick={() => missing.forEach((p) => !downloads[p!.id] && void download(p!.id))}>
                Download now
              </Button>
            )}
          </div>
        </div>
      )}
    </Modal>
  );
}
