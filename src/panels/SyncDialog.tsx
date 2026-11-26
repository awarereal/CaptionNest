import { useMemo, useState } from "react";
import { ArrowRight, CheckCircle, Clock, Desktop, Globe, WarningCircle } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { useSync } from "../state/sync";
import { sortCues } from "../lib/cueOps";
import { describeError } from "../lib/errors";
import { languageName } from "../lib/languages";
import { modelName } from "../lib/models";
import { syncWithDeepgram } from "../lib/online";
import { useVoice } from "../state/voice";
import { parseTimecode } from "../lib/time";
import type { ModelState, Stage, SyncedCue, SyncSummary, Track } from "../lib/types";
import { Button, Modal, ProgressBar } from "../components/ui";

const STAGE_TEXT: Partial<Record<Stage, string>> = {
  preparing: "Getting ready",
  extracting: "Preparing the sound",
  transcribing: "Listening to the video",
  finishing: "Matching captions to the speech",
};

/** m:ss.cc, the way positions are shown in this dialog. */
const at = (ms: number) => `${Math.floor(ms / 60000)}:${((ms % 60000) / 1000).toFixed(2).padStart(5, "0")}`;
const secs = (ms: number) => `${(Math.abs(ms) / 1000).toFixed(Math.abs(ms) < 10_000 ? 2 : 1)} s`;
/** A negative shift moves a caption earlier: it was late. */
const wasOff = (ms: number) => (Math.abs(ms) < 80 ? "in sync" : `${secs(ms)} ${ms < 0 ? "late" : "early"}`);

/** The offline model that will listen, as the app chooses it. */
function listeningModel(models: ModelState | null, track: Track | null, chosen: string | null | undefined): string | null {
  const installed = (models?.whisper ?? []).filter((m) => m.installed);
  const forLanguage = installed.filter((m) => m.language && m.language === track?.language).sort((a, b) => a.sizeBytes - b.sizeBytes)[0];
  return (forLanguage ?? installed.find((m) => m.id === "large-v3-turbo-q5_0") ?? installed.find((m) => m.id === chosen) ?? null)?.id ?? null;
}

function headline(s: SyncSummary): string {
  switch (s.kind) {
    case "in_sync":
      return "These captions are already in sync.";
    case "offset":
      return `The captions were about ${wasOff(s.medianShiftMs)}, all by the same amount.`;
    case "drift":
      return `The captions drifted: ${wasOff(s.firstShiftMs)} at the start, ${wasOff(s.lastShiftMs)} by the end.`;
    case "mixed":
      return `Captions were off by different amounts, up to ${secs(s.maxShiftMs)}.`;
    case "no_match":
      return "The words of these captions weren't heard in this video.";
  }
}

export function SyncDialog() {
  const sync = useSync();
  const tracks = useStore((s) => s.history.present);
  const project = useStore((s) => s.project);
  const models = useStore((s) => s.models);
  const settings = useStore((s) => s.settings);
  const media = useStore((s) => s.media);
  const videoMissing = useStore((s) => s.videoMissing);
  const openPanel = useStore((s) => s.openPanel);
  const deepgramKey = useVoice((s) => !!s.status?.keySaved);
  const track = tracks.find((t) => t.id === sync.trackId) ?? null;
  const open = sync.phase !== "closed";
  if (!project || !track) return null;

  const model = listeningModel(models, track, settings?.transcribeModelId);
  const online = syncWithDeepgram(settings, deepgramKey, track?.language);
  const blocked =
    track.kind === "translation"
      ? "These captions are a translation, so their words aren't the words spoken. Auto-sync the captions in the spoken language instead."
      : videoMissing
        ? "The video isn't where it was. Locate it first."
        : (media?.audioTracks.length ?? 0) === 0
          ? "This video has no sound to listen to."
          : track.cues.length === 0
            ? "There are no captions to sync yet."
            : null;

  if (sync.phase === "preview" && sync.result) {
    return <Preview track={track} />;
  }

  return (
    <Modal
      open={open}
      onOpenChange={(o) => !o && sync.close()}
      title="Auto-sync captions"
      description="CaptionNest listens to the video and moves each caption to where its words are spoken. Only the times change: the words stay exactly as they are."
      size="md"
      footer={
        sync.phase === "running" ? (
          <>
            <span className="spacer" />
            <Button variant="secondary" onClick={() => void sync.cancel()}>
              Stop
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={sync.close}>
              Cancel
            </Button>
            <span className="spacer" />
            <Button variant="primary" icon={<Clock size={16} weight="bold" />} disabled={!!blocked || (!model && !online)} onClick={() => void sync.start()}>
              {sync.phase === "failed" ? "Try again" : "Find the right times"}
            </Button>
          </>
        )
      }
    >
      {sync.phase === "running" ? (
        <div className="burnprog" aria-live="polite">
          <p className="burnprog__stage">{STAGE_TEXT[sync.stage ?? "preparing"] ?? "Working"}</p>
          <ProgressBar value={sync.stage === "transcribing" ? sync.progress : null} label="Auto-sync progress" tone="teal" />
          <p className="hint">Nothing is changed until you've seen the result.</p>
        </div>
      ) : (
        <>
          {blocked ? (
            <p className="dl__error" role="alert">
              {blocked}
            </p>
          ) : (
            <>
              <fieldset className="tracks-pick">
                <legend className="label">What to sync</legend>
                <label className={`radio-card ${sync.scope === "all" ? "is-checked" : ""}`}>
                  <input type="radio" name="sync-scope" checked={sync.scope === "all"} onChange={() => sync.setScope("all")} />
                  <span className="radio-card__title">The whole video</span>
                  <span className="radio-card__meta">
                    {track.cues.length} {languageName(track.language)} captions
                  </span>
                </label>
                <label className={`radio-card ${sync.scope === "section" ? "is-checked" : ""}`}>
                  <input type="radio" name="sync-scope" checked={sync.scope === "section"} onChange={() => sync.setScope("section")} />
                  <span className="radio-card__title">Only a section</span>
                  <span className="radio-card__meta">Captions that start inside it; the rest stay untouched</span>
                </label>
              </fieldset>
              {sync.scope === "section" && <SectionRange durationMs={project.video.durationMs} track={track} />}

              {online ? (
                <div className="engine engine--online">
                  <Globe size={18} />
                  <div className="engine__body">
                    <p className="engine__title">Listens online with Deepgram</p>
                    <p className="hint">Hears the words even over music, and every word has its own time. Uses your Deepgram credits.</p>
                  </div>
                </div>
              ) : model ? (
                <div className="engine">
                  <Desktop size={18} />
                  <div className="engine__body">
                    <p className="engine__title">Listens on this computer with the {modelName(model)} model</p>
                    <p className="hint">Free and offline. Usually takes a few seconds per minute of video.</p>
                  </div>
                </div>
              ) : (
                <div className="callout callout--gold">
                  <WarningCircle size={18} weight="fill" />
                  <div className="callout__body">
                    <p>
                      <strong>Auto-sync listens with an offline speech model, and none is downloaded yet.</strong>
                    </p>
                    <Button size="sm" variant="secondary" onClick={() => (sync.close(), openPanel("settings", "models"))}>
                      Open speech models
                    </Button>
                  </div>
                </div>
              )}
              <p className="hint syncdlg__limits">
                Captions whose words can't be heard clearly (loud music, people talking over each other) aren't guessed: they're marked for you to
                check.
              </p>
            </>
          )}
          {sync.phase === "failed" && sync.error && (
            <p className="dl__error" role="alert">
              {describeError(sync.error).message}
            </p>
          )}
        </>
      )}
    </Modal>
  );
}

function SectionRange({ durationMs, track }: { durationMs: number; track: Track }) {
  const fromMs = useSync((s) => s.fromMs);
  const toMs = useSync((s) => s.toMs);
  const setRange = useSync((s) => s.setRange);
  const [from, setFrom] = useState(at(fromMs));
  const [to, setTo] = useState(at(toMs));
  const lo = Math.min(fromMs, toMs);
  const hi = Math.max(fromMs, toMs);
  const count = track.cues.filter((c) => c.startMs >= lo && c.startMs <= hi).length;
  const commit = (which: "from" | "to", text: string) => {
    const ms = parseTimecode(text);
    if (ms === null) {
      // Not a time: put back what was there.
      if (which === "from") setFrom(at(fromMs));
      else setTo(at(toMs));
      return;
    }
    const v = Math.max(0, Math.min(durationMs, ms));
    if (which === "from") {
      setRange(v, toMs);
      setFrom(at(v));
    } else {
      setRange(fromMs, v);
      setTo(at(v));
    }
  };
  return (
    <div className="syncrange">
      <label className="syncrange__field">
        <span className="label">From</span>
        <input className="field mono" value={from} onChange={(e) => setFrom(e.target.value)} onBlur={(e) => commit("from", e.target.value)} aria-label="Section start" />
      </label>
      <label className="syncrange__field">
        <span className="label">To</span>
        <input className="field mono" value={to} onChange={(e) => setTo(e.target.value)} onBlur={(e) => commit("to", e.target.value)} aria-label="Section end" />
      </label>
      <p className="hint syncrange__count">
        {count === 0 ? "No captions start in this section." : `${count} ${count === 1 ? "caption starts" : "captions start"} in this section.`}
      </p>
    </div>
  );
}

type Filter = "all" | "moved" | "review";

function Preview({ track }: { track: Track }) {
  const sync = useSync();
  const result = sync.result!;
  const [filter, setFilter] = useState<Filter>("moved");
  const rows = useMemo(() => {
    const byId = new Map(result.cues.map((c) => [c.id, c]));
    return sortCues(track.cues)
      .map((cue, i) => ({ cue, number: i + 1, s: byId.get(cue.id) }))
      .filter((r): r is { cue: (typeof r)["cue"]; number: number; s: SyncedCue } => !!r.s && r.s.status !== "outside");
  }, [result, track]);
  const moved = rows.filter((r) => r.s.startMs !== r.s.oldStartMs || r.s.endMs !== r.s.oldEndMs);
  const review = rows.filter((r) => r.s.status === "estimated" || r.s.status === "unmatched");
  const shown = filter === "moved" ? moved : filter === "review" ? review : rows;
  const nothing = moved.length === 0 && review.length === 0;

  return (
    <Modal
      open
      onOpenChange={(o) => !o && sync.close()}
      title="Auto-sync: review before applying"
      size="lg"
      className="syncdlg"
      footer={
        <>
          <Button variant="ghost" onClick={sync.back}>
            Back
          </Button>
          <span className="spacer" />
          <Button variant="secondary" onClick={sync.close}>
            {nothing ? "Close" : "Don't change anything"}
          </Button>
          {!nothing && (
            <Button variant="primary" icon={<CheckCircle size={16} weight="bold" />} onClick={sync.apply}>
              {moved.length === 0 ? "Mark captions to review" : `Apply to ${moved.length} ${moved.length === 1 ? "caption" : "captions"}`}
            </Button>
          )}
        </>
      }
    >
      <div className={`syncsum syncsum--${result.summary.kind}`}>
        <p className="syncsum__head">{headline(result.summary)}</p>
        <p className="hint">
          {result.summary.kind === "no_match"
            ? "Check that this is the right video and sound track, and that the captions are in the language spoken."
            : `${moved.length} of ${rows.length} captions will move · ${rows.length - moved.length - review.filter((r) => r.s.startMs === r.s.oldStartMs).length} already right · ${review.length} to review`}
        </p>
        {review.length > 0 && result.summary.kind !== "no_match" && (
          <p className="hint">
            Captions to review couldn't be heard clearly. They're placed by the captions around them, or left where they were, and marked
            “Check timing” in the list.
          </p>
        )}
      </div>

      <div className="seg syncdlg__filter" role="radiogroup" aria-label="Captions shown">
        {(
          [
            ["moved", `Moving (${moved.length})`],
            ["review", `To review (${review.length})`],
            ["all", `All (${rows.length})`],
          ] as [Filter, string][]
        ).map(([id, label]) => (
          <button key={id} role="radio" aria-checked={filter === id} className={`seg__opt ${filter === id ? "is-on" : ""}`} onClick={() => setFilter(id)}>
            {label}
          </button>
        ))}
      </div>

      {shown.length === 0 ? (
        <p className="hint syncdlg__empty">{filter === "moved" ? "No caption needs to move." : filter === "review" ? "Nothing to review." : "No captions."}</p>
      ) : (
        <ol className="synclist">
          {shown.map(({ cue, number, s }) => {
            const shift = s.startMs - s.oldStartMs;
            const changed = s.startMs !== s.oldStartMs || s.endMs !== s.oldEndMs;
            return (
              <li key={cue.id} className={`syncrow ${s.status === "matched" ? "" : "is-review"}`}>
                <span className="syncrow__n mono">{number}</span>
                <span className="syncrow__text caption-text" lang={track.language}>
                  {cue.text.replace(/\n/g, " ")}
                </span>
                <span className="syncrow__times mono">
                  {at(s.oldStartMs)}
                  {changed && (
                    <>
                      <ArrowRight size={12} /> <span className="syncrow__new">{at(s.startMs)}</span>
                    </>
                  )}
                </span>
                <span className={`syncrow__shift mono ${changed ? "" : "is-same"}`}>
                  {Math.abs(shift) >= 10 ? `${shift > 0 ? "+" : "−"}${secs(shift)}` : changed ? "length" : "—"}
                </span>
                <span className={`syncrow__status ${s.status === "matched" ? "" : "is-review"}`}>
                  {s.status === "matched" ? (changed ? "Moved" : "Already right") : s.status === "estimated" ? "Placed by neighbours" : "Not heard, left as is"}
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </Modal>
  );
}
