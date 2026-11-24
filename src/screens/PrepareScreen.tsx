import { useEffect, useMemo, useState } from "react";
import {
  Subtitles,
  Info,
  WarningCircle,
  DownloadSimple,
  Check,
  CircleNotch,
  Circle,
  Clock,
  ArrowClockwise,
  FilmSlate,
  SlidersHorizontal,
  Translate,
  Waveform,
} from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { Player } from "../components/Player";
import { LanguagePicker, type PickerItem } from "../components/LanguagePicker";
import { Badge, Button, ProgressBar, Select, formatBytes } from "../components/ui";
import { SPOKEN_LANGUAGES, TIER_LABEL, language, languageName } from "../lib/languages";
import { MODEL_INFO, languageModelFor, modelName } from "../lib/models";
import { formatDuration } from "../lib/time";
import { api } from "../lib/api";
import { describeError, type RecoveryAction } from "../lib/errors";
import { browseVideo } from "../lib/fileFlows";
import type { AppError, AudioTrack, PlanInfo, Stage } from "../lib/types";
import { DownloadStatus } from "../components/ModelCard";

const FEATURED = ["en", "bn", "hi", "ar"];

export function PrepareScreen() {
  const media = useStore((s) => s.media);
  const job = useStore((s) => s.job);
  const jobError = useStore((s) => s.jobError);
  if (!media) return null;
  return (
    <div className="prepare">
      <section className="prepare__media">
        <Player durationMs={media.durationMs} />
        <MediaFacts />
      </section>
      <aside className="prepare__panel">
        {job ? <JobProgress /> : jobError ? <JobError error={jobError} /> : <CaptionOptions />}
      </aside>
    </div>
  );
}

function MediaFacts() {
  const media = useStore((s) => s.media)!;
  const v = media.video;
  return (
    <dl className="facts">
      <div>
        <dt>Length</dt>
        <dd>{formatDuration(media.durationMs)}</dd>
      </div>
      {v && (
        <div>
          <dt>Picture</dt>
          <dd>
            {v.width}×{v.height}
            {v.frameRate ? ` · ${Math.round(v.frameRate * 100) / 100} fps` : ""}
            {v.variableFrameRate ? " (variable)" : ""}
          </dd>
        </div>
      )}
      <div>
        <dt>Sound</dt>
        <dd>
          {media.audioTracks.length === 0
            ? "None"
            : media.audioTracks.length === 1
              ? trackLabel(media.audioTracks[0], 0)
              : `${media.audioTracks.length} tracks`}
        </dd>
      </div>
      <div>
        <dt>Size</dt>
        <dd>{formatBytes(media.sizeBytes)}</dd>
      </div>
    </dl>
  );
}

function trackLabel(t: AudioTrack, i: number) {
  const parts = [t.title || `Track ${i + 1}`];
  if (t.language) parts.push(languageName(t.language.length === 3 ? t.language.slice(0, 2) : t.language, t.language));
  parts.push(t.channels === 1 ? "mono" : t.channels === 2 ? "stereo" : `${t.channels} ch`);
  return parts.join(" · ");
}

function tierBadge(code: string) {
  const l = language(code);
  if (!l) return null;
  const tone = l.tier === "good" ? "ok" : l.tier === "fair" ? "gold" : "danger";
  return <Badge tone={tone}>{TIER_LABEL[l.tier]}</Badge>;
}

function CaptionOptions() {
  const media = useStore((s) => s.media)!;
  const settings = useStore((s) => s.settings)!;
  const models = useStore((s) => s.models);
  const audioStream = useStore((s) => s.audioStream);
  const setAudioStream = useStore((s) => s.setAudioStream);
  const update = useStore((s) => s.updateSettings);
  const start = useStore((s) => s.startGenerate);
  const download = useStore((s) => s.download);
  const openPanel = useStore((s) => s.openPanel);
  const downloads = useStore((s) => s.downloads);
  const [plan, setPlan] = useState<PlanInfo | null>(null);

  const spoken = settings.spokenLanguage;
  const caption = settings.captionLanguage;
  const installedWhisper = (models?.whisper ?? []).filter((m) => m.installed);
  const generalInstalled = installedWhisper.filter((m) => !m.language);
  const model = installedWhisper.find((m) => m.id === settings.transcribeModelId && !m.language) ?? installedWhisper.find((m) => !m.language);
  const translator = installedWhisper.find((m) => MODEL_INFO[m.id]?.translates);
  const accurate = models?.whisper.find((m) => m.id === "large-v3-q5_0");
  const banglaModel = languageModelFor(models?.whisper, "bn");

  // Argos targets: languages captions can be translated into.
  const argosTargets = useMemo(() => {
    const map = new Map<string, { installed: boolean; size: number; name: string }>();
    for (const p of models?.argos ?? []) {
      if (p.from !== "en") continue;
      map.set(p.to, { installed: p.installed, size: p.sizeBytes, name: p.toName ?? p.to });
    }
    return map;
  }, [models]);

  const spokenItems: PickerItem[] = useMemo(() => {
    const auto: PickerItem = { value: "auto", label: "Detect automatically", group: "Automatic", meta: "Works best with one main language" };
    const toItem = (group: string) => (l: (typeof SPOKEN_LANGUAGES)[number]): PickerItem => ({
      value: l.code,
      label: l.name,
      native: l.native,
      group,
      keywords: l.code === "bn" ? ["bengali"] : [],
      badge: tierBadge(l.code),
    });
    const featured = FEATURED.map((c) => language(c)!).map(toItem("Common"));
    const rest = SPOKEN_LANGUAGES.filter((l) => !FEATURED.includes(l.code))
      .sort((a, b) => a.name.localeCompare(b.name))
      .map(toItem("All languages"));
    return [auto, ...featured, ...rest];
  }, []);

  const captionItems: PickerItem[] = useMemo(() => {
    const items: PickerItem[] = [
      { value: "same", label: "Same as speech", group: "Captions", meta: "No translation" },
      {
        value: "en",
        label: "English",
        group: "Captions",
        meta: translator ? "Translated from the speech" : "Needs the Accurate model",
      },
    ];
    const ready: PickerItem[] = [];
    const later: PickerItem[] = [];
    for (const [code, t] of argosTargets) {
      const lang = language(code);
      const item: PickerItem = {
        value: code,
        label: lang?.name ?? t.name,
        native: lang?.native,
        group: t.installed ? "Ready offline" : "Download to use",
        meta: t.installed ? undefined : formatBytes(t.size),
        keywords: code === "bn" ? ["bengali"] : [],
      };
      (t.installed ? ready : later).push(item);
    }
    const byName = (a: PickerItem, b: PickerItem) => a.label.localeCompare(b.label);
    return [...items, ...ready.sort(byName), ...later.sort(byName)];
  }, [argosTargets, translator]);

  // Ask the backend which packages a translation needs.
  useEffect(() => {
    let alive = true;
    if (caption === "same" || caption === "en" || caption === spoken) {
      setPlan(null);
      return;
    }
    const source = spoken === "auto" ? "en" : spoken;
    api.planTranslation(source, caption).then((p) => alive && setPlan(p));
    return () => {
      alive = false;
    };
  }, [caption, spoken, models]);

  const spokenLang = language(spoken);
  const missingPacks = (plan?.missingPackages ?? []).map((id) => models?.argos.find((p) => p.id === id)).filter(Boolean);
  const needsTranslator = caption === "en" && spoken !== "en" && !translator;
  const noModel = !model;
  const blocked = noModel || needsTranslator || missingPacks.length > 0 || audioStream === null;

  return (
    <div className="options">
      <header className="panel-head">
        <h2 className="panel-head__title display">Captions</h2>
        <p className="panel-head__sub">Choose the language, then generate. You'll review everything before exporting.</p>
      </header>

      {media.audioTracks.length === 0 ? (
        <div className="callout callout--danger">
          <WarningCircle size={18} weight="fill" />
          <div>
            <strong>This file has no sound.</strong> There's no speech to turn into captions. Choose a different video.
          </div>
        </div>
      ) : (
        media.audioTracks.length > 1 && (
          <div className="formrow">
            <label className="label" htmlFor="track">
              <Waveform size={14} /> Sound track
            </label>
            <Select
              id="track"
              label="Sound track"
              value={String(audioStream ?? "")}
              onChange={(v) => setAudioStream(Number(v))}
              options={media.audioTracks.map((t, i) => ({
                value: String(t.streamIndex),
                label: trackLabel(t, i),
                hint: t.isDefault ? "Default track" : t.codec.toUpperCase(),
              }))}
            />
          </div>
        )
      )}

      <div className="formrow">
        <label className="label" htmlFor="spoken">
          Spoken language
        </label>
        <LanguagePicker id="spoken" label="Spoken language" value={spoken} items={spokenItems} onChange={(v) => void update({ spokenLanguage: v })} />
        {spokenLang?.note && (
          <p className="formrow__note">
            <Info size={14} /> {spokenLang.note}
          </p>
        )}
        {spoken === "auto" &&
          (accurate?.installed ? (
            <p className="formrow__note">Detection listens to the first 30 seconds of speech. Choose the language if more than one is spoken.</p>
          ) : (
            <p className="formrow__note formrow__note--warn">
              <WarningCircle size={14} /> Without the Accurate model, automatic detection often mistakes Bangla and other South Asian languages.
              Choose the spoken language for reliable results.
            </p>
          ))}
      </div>

      <div className="formrow">
        <label className="label" htmlFor="caption">
          Caption language
        </label>
        <LanguagePicker id="caption" label="Caption language" value={caption} items={captionItems} onChange={(v) => void update({ captionLanguage: v })} />
        {caption !== "same" && caption !== "en" && caption !== spoken && (
          <p className="formrow__note">
            <Translate size={14} /> Translated offline from the transcript, keeping every caption's timing.
            {plan?.plan?.kind === "text" && plan.plan.packages.length > 1 && " Goes through English, so wording can drift."}
          </p>
        )}
        {caption === "en" && spoken !== "en" && (
          <p className="formrow__note">
            <Translate size={14} /> Whisper translates the speech straight into English. Meaning comes through; wording is freer than a human
            translation.
          </p>
        )}
      </div>

      {needsTranslator && accurate && (
        <div className="callout callout--gold">
          <DownloadSimple size={18} />
          <div className="callout__body">
            <p>
              <strong>English captions from other languages need the Accurate model</strong> ({formatBytes(accurate.sizeBytes)}).
            </p>
            {downloads[accurate.id] ? (
              <DownloadStatus id={accurate.id} total={accurate.sizeBytes} partial={accurate.partialBytes} />
            ) : (
              <Button size="sm" variant="secondary" icon={<DownloadSimple size={15} />} onClick={() => void download(accurate.id)}>
                Download now
              </Button>
            )}
          </div>
        </div>
      )}

      {missingPacks.length > 0 && (
        <div className="callout callout--gold">
          <DownloadSimple size={18} />
          <div className="callout__body">
            <p>
              <strong>{languageName(caption)} captions need {missingPacks.length === 1 ? "a language pack" : `${missingPacks.length} language packs`}</strong> (
              {formatBytes(missingPacks.reduce((a, p) => a + (p?.sizeBytes ?? 0), 0))}).
            </p>
            {missingPacks.map((p) =>
              downloads[p!.id] ? (
                <DownloadStatus key={p!.id} id={p!.id} total={p!.sizeBytes} />
              ) : null,
            )}
            {missingPacks.some((p) => !downloads[p!.id]) && (
              <Button
                size="sm"
                variant="secondary"
                icon={<DownloadSimple size={15} />}
                onClick={() => missingPacks.forEach((p) => !downloads[p!.id] && void download(p!.id))}
              >
                Download now
              </Button>
            )}
            {spoken === "auto" && <p className="hint">If the speech isn't English, another pack may be needed once the language is detected.</p>}
          </div>
        </div>
      )}

      {(spoken === "bn" || spoken === "auto") && banglaModel && !banglaModel.installed && (
        <div className="callout callout--gold">
          <DownloadSimple size={18} />
          <div className="callout__body">
            <p>
              <strong>{spoken === "bn" ? "For correct Bangla spelling, add the Bangla model" : "Bangla video? Add the Bangla model for correct spelling"}</strong> (
              {formatBytes(banglaModel.sizeBytes)}). It's used automatically whenever the speech is Bangla.
            </p>
            {downloads[banglaModel.id] ? (
              <DownloadStatus id={banglaModel.id} total={banglaModel.sizeBytes} partial={banglaModel.partialBytes} />
            ) : (
              <Button size="sm" variant="secondary" icon={<DownloadSimple size={15} />} onClick={() => void download(banglaModel.id)}>
                {banglaModel.partialBytes > 0 ? "Resume download" : "Download"}
              </Button>
            )}
          </div>
        </div>
      )}

      <div className="options__model">
        <span className="options__model-label">
          <SlidersHorizontal size={15} /> Speech model
        </span>
        {generalInstalled.length > 1 ? (
          <Select
            label="Speech model"
            value={model?.id ?? ""}
            onChange={(v) => void update({ transcribeModelId: v })}
            options={installedWhisper.filter((m) => !m.language).map((m) => ({ value: m.id, label: modelName(m.id), hint: MODEL_INFO[m.id]?.bestFor }))}
          />
        ) : (
          <span className="options__model-name">{model ? modelName(model.id) : "None installed"}</span>
        )}
        {(spoken === "bn" || spoken === "auto") && banglaModel?.installed && (
          <p className="formrow__note">
            <Info size={14} /> Bangla speech uses the Bangla model for correct spelling.
          </p>
        )}
        {(spoken === "bn" || spoken === "as") && model?.id === "small" && !banglaModel?.installed && (
          <p className="formrow__note formrow__note--warn">
            <WarningCircle size={14} /> The Light model can't write Bengali script. Use Fast or Accurate for Bangla.
          </p>
        )}
      </div>

      {noModel && (
        <div className="callout callout--danger">
          <WarningCircle size={18} weight="fill" />
          <div>
            <strong>No speech model is installed.</strong>{" "}
            <button className="btn btn--quiet" onClick={() => openPanel("settings", "models")}>
              Download one in Settings
            </button>
          </div>
        </div>
      )}

      <div className="options__go">
        <Button variant="primary" size="lg" icon={<Subtitles size={20} weight="bold" />} onClick={() => void start()} disabled={blocked}>
          Generate captions
        </Button>
        <p className="hint">Runs on this computer. You can stop at any time.</p>
      </div>
    </div>
  );
}

const STAGE_TEXT: Record<Stage, string> = {
  preparing: "Getting ready",
  extracting: "Preparing audio",
  transcribing: "Recognizing speech",
  translating: "Translating",
  finishing: "Building captions",
  converting: "Converting",
};

function Elapsed({ since }: { since: number }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);
  const s = Math.floor((Date.now() - since) / 1000);
  return (
    <span className="mono">
      {Math.floor(s / 60)}:{String(s % 60).padStart(2, "0")}
    </span>
  );
}

/** Estimate from this stage's own pace; shown once there's enough to go on. */
function TimeLeft({ since, progress }: { since: number; progress: number | null }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 2000);
    return () => clearInterval(t);
  }, []);
  const elapsed = (Date.now() - since) / 1000;
  if (progress === null || progress < 0.04 || progress >= 1 || elapsed < 8) return null;
  const left = (elapsed * (1 - progress)) / progress;
  const text = left < 50 ? "Less than a minute left" : left < 90 ? "About a minute left" : `About ${Math.round(left / 60)} min left`;
  return <span className="stage__eta">{text}</span>;
}

export function JobProgress({ compact = false }: { compact?: boolean }) {
  const job = useStore((s) => s.job)!;
  const settings = useStore((s) => s.settings);
  const cancel = useStore((s) => s.cancelJob);
  const [stopping, setStopping] = useState(false);
  const order: Stage[] = [...job.stages, "finishing"];
  const currentIdx = job.stage === "preparing" ? -1 : order.indexOf(job.stage);
  const target = job.kind === "translate" ? job.targetLanguage : settings?.captionLanguage;

  const label = (s: Stage) => {
    if (s === "transcribing" && settings && settings.spokenLanguage !== "auto") return `Recognizing ${languageName(settings.spokenLanguage)} speech`;
    if (s === "translating" && target && target !== "same") return `Translating to ${languageName(target)}`;
    return STAGE_TEXT[s];
  };

  return (
    <div className={`jobp ${compact ? "jobp--compact" : ""}`} aria-live="polite">
      <header className="panel-head">
        <h2 className="panel-head__title display">{job.kind === "translate" ? "Adding a caption language" : "Making captions"}</h2>
        <p className="panel-head__sub">
          <Clock size={14} /> <Elapsed since={job.startedAt} /> elapsed · keep CaptionNest open
        </p>
      </header>
      <ol className="stages">
        {order.map((s, i) => {
          const state = i < currentIdx ? "done" : i === currentIdx ? "active" : "todo";
          return (
            <li key={s} className={`stage stage--${state}`}>
              <span className="stage__icon" aria-hidden>
                {state === "done" ? <Check size={14} weight="bold" /> : state === "active" ? <CircleNotch size={16} className="spin" /> : <Circle size={10} weight="fill" />}
              </span>
              <div className="stage__body">
                <span className="stage__label">
                  {label(s)}
                  <span className="visually-hidden">{state === "done" ? " — done" : state === "active" ? " — in progress" : " — waiting"}</span>
                </span>
                {state === "active" && (
                  <>
                    <div className="stage__progress">
                      <ProgressBar value={job.progress} label={label(s)} />
                      <span className="mono stage__pct">{job.progress !== null ? `${Math.round(job.progress * 100)}%` : ""}</span>
                    </div>
                    <TimeLeft since={job.stageStartedAt} progress={job.progress} />
                  </>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      {job.stage === "transcribing" && (job.progress ?? 0) < 0.02 && (
        <p className="hint jobp__hint">Loading the speech model can take a few seconds the first time.</p>
      )}
      <footer className="jobp__foot">
        <Button
          variant="secondary"
          busy={stopping}
          onClick={async () => {
            setStopping(true);
            await cancel();
            setStopping(false);
          }}
        >
          Stop
        </Button>
      </footer>
    </div>
  );
}

export function JobError({ error }: { error: AppError }) {
  const d = describeError(error);
  const [details, setDetails] = useState(false);
  const st = useStore.getState();
  const act = (a: RecoveryAction) => {
    switch (a) {
      case "retry":
        st.dismissJobError();
        if (st.view === "workspace") return;
        void st.startGenerate();
        break;
      case "choose-video":
        void browseVideo();
        break;
      case "choose-track":
      case "set-language":
        st.dismissJobError();
        break;
      case "open-models":
      case "free-space":
        st.openPanel("settings", "models");
        st.dismissJobError();
        break;
      case "open-languages":
        st.openPanel("settings", "languages");
        st.dismissJobError();
        break;
      case "show-details":
        setDetails((v) => !v);
        break;
      case "preview":
        void st.requestPreview();
        break;
    }
  };
  const LABELS: Record<RecoveryAction, string> = {
    retry: "Try again",
    "choose-video": "Choose another video",
    "choose-track": "Choose another sound track",
    "open-models": "Open speech models",
    "open-languages": "Open language packs",
    preview: "Make a preview copy",
    "set-language": "Choose the spoken language",
    "free-space": "Manage downloads",
    "show-details": details ? "Hide details" : "Show details",
  };
  return (
    <div className="joberr" role="alert">
      <span className="joberr__icon">
        <WarningCircle size={26} weight="fill" />
      </span>
      <h2 className="joberr__title display">{d.title}</h2>
      <p className="joberr__msg">{d.message}</p>
      <div className="joberr__actions">
        {d.actions.map((a, i) => (
          <Button
            key={a}
            variant={i === 0 ? "primary" : a === "show-details" ? "ghost" : "secondary"}
            icon={a === "retry" ? <ArrowClockwise size={16} /> : a === "choose-video" ? <FilmSlate size={16} /> : undefined}
            onClick={() => act(a)}
          >
            {LABELS[a]}
          </Button>
        ))}
        {!d.actions.includes("retry") && (
          <Button variant="ghost" onClick={() => st.dismissJobError()}>
            Back to options
          </Button>
        )}
      </div>
      {details && error.detail && <pre className="joberr__detail selectable">{error.detail}</pre>}
    </div>
  );
}

