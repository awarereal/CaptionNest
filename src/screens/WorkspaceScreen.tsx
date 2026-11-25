import { useEffect, useMemo, useState } from "react";
import { Popover, DropdownMenu } from "radix-ui";
import {
  Plus,
  ArrowCounterClockwise,
  ArrowClockwise,
  Clock,
  Translate,
  DotsThree,
  MagicWand,
  Trash,
  FolderOpen,
  Info,
  X,
  WarningCircle,
  Metronome,
} from "@phosphor-icons/react";
import { toast } from "sonner";
import { useActiveTrack, useStore } from "../state/store";
import { usePlayhead } from "../state/playhead";
import { Player } from "../components/Player";
import { CueList } from "../components/CueList";
import { Button, IconButton } from "../components/ui";
import { canRedo, canUndo } from "../lib/history";
import { languageName } from "../lib/languages";
import { overlappingIds, sortCues, wordsToCheck, type ShiftScope } from "../lib/cueOps";
import { parseOffsetSeconds } from "../lib/time";
import { api, toAppError } from "../lib/api";
import { confirmDiscard, pickOpen, saveProject } from "../lib/fileFlows";
import { useExport } from "../panels/ExportDialog";
import { ExportDialog } from "../panels/ExportDialog";
import { TranslateDialog } from "../panels/TranslateDialog";
import { StylePanel } from "../panels/StylePanel";
import { SyncDialog } from "../panels/SyncDialog";
import { useSync } from "../state/sync";
import { JobError, JobProgress } from "./PrepareScreen";
import { VIDEO_EXTENSIONS } from "../lib/fileFlows";
import { ask } from "../components/Confirm";
import { BanglaTypingToggle } from "../components/TypingAids";
import { useBanglaTyping } from "../components/usePhonetic";

export function WorkspaceScreen() {
  const project = useStore((s) => s.project);
  const track = useActiveTrack();
  const job = useStore((s) => s.job);
  const jobError = useStore((s) => s.jobError);
  const videoMissing = useStore((s) => s.videoMissing);
  const warnings = useStore((s) => s.warnings);
  const styleOpen = useStore((s) => s.styleOpen);
  const [translateOpen, setTranslateOpen] = useState(false);
  useShortcuts();

  if (!project) return null;
  return (
    <div className="ws">
      <section className="ws__video">
        {videoMissing && <RelinkBanner />}
        <UncertainLanguage />
        <UnrecognizedSpeech />
        {warnings.includes("gpu_fallback") && (
          <Banner tone="teal" onClose={() => useStore.setState({ warnings: warnings.filter((w) => w !== "gpu_fallback") })}>
            Graphics acceleration wasn't available, so this run used the processor. Results are the same; it just took longer.
          </Banner>
        )}
        {warnings.some((w) => w.startsWith("online_fallback:")) && (
          <Banner
            tone="gold"
            onClose={() => useStore.setState({ warnings: warnings.filter((w) => !w.startsWith("online_fallback:")) })}
            action={
              <Button size="sm" variant="secondary" onClick={() => useStore.getState().openPanel("settings", "online")}>
                Online settings
              </Button>
            }
          >
            Online recognition didn't work this time ({warnings.find((w) => w.startsWith("online_fallback:"))!.slice("online_fallback:".length)}), so
            these captions were made with the offline model.
          </Banner>
        )}
        {warnings.includes("online_unsupported_language") && (
          <Banner tone="teal" onClose={() => useStore.setState({ warnings: warnings.filter((w) => w !== "online_unsupported_language") })}>
            Sarvam doesn't recognize this language, so these captions were made with the offline model.
          </Banner>
        )}
        <Player cues={track?.cues ?? []} showLane durationMs={project.video.durationMs} />
      </section>
      {styleOpen ? (
        <section className="ws__captions ws__captions--style">
          <StylePanel />
        </section>
      ) : (
        <section className="ws__captions" aria-label="Caption editor">
          <CaptionsHeader onTranslate={() => setTranslateOpen(true)} />
          <div className="ws__list">
            {job ? (
              <div className="ws__job">
                <JobProgress compact />
              </div>
            ) : jobError ? (
              <div className="ws__job">
                <JobError error={jobError} />
              </div>
            ) : track ? (
              <CueList cues={track.cues} language={track.language} />
            ) : null}
          </div>
          <StatusBar />
        </section>
      )}
      <ExportDialog />
      <SyncDialog />
      <TranslateDialog open={translateOpen} onOpenChange={setTranslateOpen} />
    </div>
  );
}

function Banner({ tone, children, onClose, action }: { tone: "gold" | "teal" | "danger"; children: React.ReactNode; onClose?: () => void; action?: React.ReactNode }) {
  return (
    <div className={`banner banner--${tone}`} role="status">
      {tone === "danger" ? <WarningCircle size={17} weight="fill" /> : <Info size={17} />}
      <span className="banner__text">{children}</span>
      {action}
      {onClose && (
        <IconButton label="Dismiss" size="sm" onClick={onClose}>
          <X size={14} />
        </IconButton>
      )}
    </div>
  );
}

/** Speech the recognizer couldn't turn into words is kept as empty captions to review. */
function UnrecognizedSpeech() {
  const warnings = useStore((s) => s.warnings);
  const track = useActiveTrack();
  const w = warnings.find((x) => x.startsWith("unrecognized:"));
  const empty = track?.kind === "original" ? sortCues(track.cues).filter((c) => !c.text.trim()) : [];
  if (!w || empty.length === 0) return null;
  const show = () => {
    useStore.getState().selectCue(empty[0].id);
    usePlayhead.getState().seek(empty[0].startMs);
  };
  return (
    <Banner
      tone="gold"
      onClose={() => useStore.setState({ warnings: warnings.filter((x) => x !== w) })}
      action={
        <Button size="sm" variant="secondary" onClick={show}>
          Show
        </Button>
      }
    >
      {empty.length === 1 ? "One stretch of speech" : `${empty.length} stretches of speech`} couldn't be recognized. They're kept as empty
      captions so nothing is missed: listen and type the words, or delete them.
    </Banner>
  );
}

/** Shown when automatic detection wasn't confident about the spoken language. */
function UncertainLanguage() {
  const warnings = useStore((s) => s.warnings);
  const media = useStore((s) => s.media);
  const w = warnings.find((x) => x.startsWith("language_uncertain:"));
  if (!w || !media) return null;
  const [, code] = w.split(":");
  const redo = async () => {
    if (!(await confirmDiscard("Going back discards these captions unless you save them."))) return;
    useStore.getState().backToOptions();
  };
  return (
    <Banner
      tone="gold"
      onClose={() => useStore.setState({ warnings: warnings.filter((x) => x !== w) })}
      action={
        <Button size="sm" variant="secondary" onClick={() => void redo()}>
          Choose language
        </Button>
      }
    >
      CaptionNest wasn't sure which language is spoken and guessed <strong>{languageName(code)}</strong>. If that's wrong, choose the spoken
      language and generate again.
    </Banner>
  );
}

function RelinkBanner() {
  const project = useStore((s) => s.project)!;
  const relink = useStore((s) => s.relinkVideo);
  const locate = async () => {
    const picked = await pickOpen({
      title: `Locate “${project.video.fileName}”`,
      filters: [{ name: "Video and audio", extensions: VIDEO_EXTENSIONS }],
    });
    if (typeof picked !== "string") return;
    try {
      const r = await relink(picked);
      if (r.durationMismatch) {
        toast("Linked, but the length differs", {
          description: "This file isn't the same length as the original. Check that captions still line up.",
        });
      } else toast("Video found", { description: "Playback works again." });
    } catch (e) {
      toast.error("Couldn't use that file", { description: toAppError(e).message, className: "toast toast--error" });
    }
  };
  return (
    <Banner
      tone="danger"
      action={
        <Button size="sm" variant="secondary" icon={<FolderOpen size={15} />} onClick={() => void locate()}>
          Locate video
        </Button>
      }
    >
      <strong>{project.video.fileName}</strong> was moved or renamed.
    </Banner>
  );
}

function trackLabel(t: { language: string; kind: string }) {
  return `${languageName(t.language)} · ${t.kind === "original" ? "Original" : "Translation"}`;
}

function CaptionsHeader({ onTranslate }: { onTranslate: () => void }) {
  const tracks = useStore((s) => s.history.present);
  const activeId = useStore((s) => s.activeTrackId);
  const setActive = useStore((s) => s.setActiveTrack);
  const history = useStore((s) => s.history);
  const undo = useStore((s) => s.undo);
  const redo = useStore((s) => s.redo);
  const addCue = useStore((s) => s.addCue);
  const deleteTrack = useStore((s) => s.deleteTrack);
  const replace = useStore((s) => s.replaceCues);
  const job = useStore((s) => s.job);
  const track = useActiveTrack();

  const fixOverlaps = async () => {
    if (!track) return;
    const fixed = await api.fixOverlaps(track.cues);
    replace(fixed);
    toast("Overlaps fixed", { description: "Each overlapping caption now ends where the next one starts." });
  };

  return (
    <header className="caphead">
      <div className="caphead__tracks" role="tablist" aria-label="Caption tracks">
        {tracks.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={t.id === activeId}
            className={`tab ${t.id === activeId ? "is-active" : ""}`}
            onClick={() => setActive(t.id)}
          >
            <span className="tab__lang">{languageName(t.language)}</span>
            <span className="tab__kind">{t.kind === "original" ? "Original" : "Translation"}</span>
          </button>
        ))}
        <Button size="sm" variant="ghost" icon={<Translate size={16} />} onClick={onTranslate} disabled={!!job}>
          Add language
        </Button>
      </div>
      <div className="caphead__tools">
        <Button size="sm" variant="secondary" icon={<Plus size={15} weight="bold" />} onClick={() => addCue(usePlayhead.getState().ms)} disabled={!track || !!job}>
          Add caption
        </Button>
        <ShiftTiming disabled={!track || !!job} />
        <Button
          size="sm"
          variant="ghost"
          icon={<Metronome size={15} />}
          onClick={() => useSync.getState().open()}
          disabled={!track || !!job}
          aria-label="Auto-sync"
          title="Auto-sync"
          className="caphead__tool"
        >
          Auto-sync
        </Button>
        <span className="caphead__spacer" />
        {track?.language === "bn" && <BanglaTypingToggle />}
        <IconButton label="Undo" shortcut="Mod+Z" onClick={undo} disabled={!canUndo(history) || !!job}>
          <ArrowCounterClockwise size={18} />
        </IconButton>
        <IconButton label="Redo" shortcut="Shift+Mod+Z" onClick={redo} disabled={!canRedo(history) || !!job}>
          <ArrowClockwise size={18} />
        </IconButton>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button className="iconbtn iconbtn--md" aria-label="More caption actions" disabled={!track || !!job}>
              <DotsThree size={20} weight="bold" />
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="menu" align="end" sideOffset={6}>
              <div className="menu__viewport">
                <DropdownMenu.Item className="menu__item" onSelect={() => void fixOverlaps()}>
                  <span className="menu__check">
                    <MagicWand size={15} />
                  </span>
                  Fix all overlaps
                </DropdownMenu.Item>
                <DropdownMenu.Item className="menu__item" onSelect={() => void saveProject(true)}>
                  <span className="menu__check">
                    <FolderOpen size={15} />
                  </span>
                  Save project as…
                </DropdownMenu.Item>
                {track && track.kind !== "original" && (
                  <DropdownMenu.Item
                    className="menu__item menu__item--danger"
                    onSelect={async () => {
                      const c = await ask({
                        title: `Remove the ${languageName(track.language)} captions?`,
                        message: "This removes the translated track from the project. You can undo this.",
                        actions: [
                          { id: "cancel", label: "Cancel", variant: "ghost" },
                          { id: "remove", label: "Remove track", variant: "danger" },
                        ],
                      });
                      if (c === "remove") deleteTrack(track.id);
                    }}
                  >
                    <span className="menu__check">
                      <Trash size={15} />
                    </span>
                    Remove {trackLabel(track)}
                  </DropdownMenu.Item>
                )}
              </div>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
    </header>
  );
}

function ShiftTiming({ disabled }: { disabled: boolean }) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const [scope, setScope] = useState<ShiftScope>("all");
  const offset = useStore((s) => s.offsetCues);
  const track = useActiveTrack();
  const selectedId = useStore((s) => s.selectedCueId);
  const sorted = useMemo(() => (track ? sortCues(track.cues) : []), [track]);
  const index = sorted.findIndex((c) => c.id === selectedId);
  const selected = index >= 0 ? sorted[index] : null;
  const number = index + 1;
  const ms = parseOffsetSeconds(value);
  const useScope: ShiftScope = selected ? scope : "all";

  // Opening with a caption selected: the paused position is most likely where it belongs.
  const onOpenChange = (o: boolean) => {
    setOpen(o);
    if (o) {
      setValue("");
      setScope(selected ? "one" : "all");
    }
  };
  const apply = () => {
    if (ms === null || ms === 0) return;
    offset(ms, useScope, selected?.id ?? null);
    setOpen(false);
    const what = useScope === "all" ? "All captions" : useScope === "from" ? `Caption ${number} and later ones` : `Caption ${number}`;
    toast(`${what} moved ${ms > 0 ? "later" : "earlier"} by ${Math.abs(ms / 1000)} s`);
  };
  return (
    <Popover.Root open={open} onOpenChange={onOpenChange}>
      <Popover.Trigger asChild>
        <Button size="sm" variant="ghost" icon={<Clock size={15} />} disabled={disabled} aria-label="Shift timing" title="Shift timing" className="caphead__tool">
          Shift timing
        </Button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="menu popover popover--shift" sideOffset={6} align="start" collisionPadding={10}>
          <form
            className="shift"
            onSubmit={(e) => {
              e.preventDefault();
              apply();
            }}
          >
            <span className="label" id="shift-scope">
              Move
            </span>
            <div className="seg shift__scope" role="radiogroup" aria-labelledby="shift-scope">
              {(
                [
                  ["all", "All captions"],
                  ["from", selected ? `${number} and later` : "From a caption on"],
                  ["one", selected ? `Only ${number}` : "One caption"],
                ] as [ShiftScope, string][]
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  role="radio"
                  aria-checked={useScope === id}
                  className={`seg__opt ${useScope === id ? "is-on" : ""}`}
                  disabled={id !== "all" && !selected}
                  onClick={() => setScope(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            {!selected && <p className="hint">Click a caption first to move only it, or it and the ones after it.</p>}

            <label className="label shift__by" htmlFor="shift-input">
              By
            </label>
            <div className="shift__row">
              <input
                id="shift-input"
                className="field mono"
                placeholder="+0.5"
                value={value}
                onChange={(e) => setValue(e.target.value)}
                autoFocus
                inputMode="decimal"
              />
              <span className="shift__unit">seconds</span>
            </div>
            <p className="hint">Use a minus sign to move captions earlier, e.g. -0.25.</p>

            {selected && <PausedDifference startMs={selected.startMs} number={number} onUse={setValue} />}

            <div className="shift__actions">
              <Button type="submit" variant="primary" size="sm" disabled={ms === null || ms === 0}>
                Apply
              </Button>
            </div>
          </form>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** Works out how far a caption's start is from where the video is now. */
function PausedDifference({ startMs, number, onUse }: { startMs: number; number: number; onUse: (value: string) => void }) {
  const now = usePlayhead((s) => s.ms);
  const diff = Math.round(now - startMs);
  if (Math.abs(diff) < 10) return null;
  const at = (v: number) => `${Math.floor(v / 60000)}:${((v % 60000) / 1000).toFixed(2).padStart(5, "0")}`;
  const signed = `${diff > 0 ? "+" : "-"}${(Math.abs(diff) / 1000).toFixed(2)}`;
  return (
    <div className="shift__diff">
      <p>
        Caption {number} starts at <span className="mono">{at(startMs)}</span>. The video is at <span className="mono">{at(now)}</span>.
      </p>
      <Button type="button" size="sm" variant="secondary" onClick={() => onUse(signed)}>
        Use the difference ({signed} s)
      </Button>
    </div>
  );
}

function StatusBar() {
  const track = useActiveTrack();
  const select = useStore((s) => s.selectCue);
  const replace = useStore((s) => s.replaceCues);
  const stats = useMemo(() => {
    const cues = track ? sortCues(track.cues) : [];
    const overlaps = overlappingIds(cues);
    const empty = cues.filter((c) => !c.text.trim());
    const invalid = cues.filter((c) => c.endMs <= c.startMs);
    const toCheck = cues.filter((c) => wordsToCheck(c).length > 0 || c.timingCheck);
    return { count: cues.length, overlaps, empty, invalid, cues, toCheck };
  }, [track]);
  if (!track) return null;
  const firstOverlap = stats.cues.find((c) => stats.overlaps.has(c.id));
  const problems = [...stats.empty, ...stats.invalid];
  return (
    <footer className="statusbar">
      <span className="mono">{stats.count} captions</span>
      {stats.overlaps.size > 0 && (
        <>
          <span className="statusbar__dot" />
          <button className="statusbar__issue" onClick={() => firstOverlap && (select(firstOverlap.id), usePlayhead.getState().seek(firstOverlap.startMs))}>
            {Math.ceil(stats.overlaps.size / 2)} overlapping
          </button>
          <button
            className="btn btn--quiet"
            onClick={async () => {
              replace(await api.fixOverlaps(track.cues));
            }}
          >
            Fix
          </button>
        </>
      )}
      {problems.length > 0 && (
        <>
          <span className="statusbar__dot" />
          <button className="statusbar__issue" onClick={() => (select(problems[0].id), usePlayhead.getState().seek(problems[0].startMs))}>
            {problems.length} need attention
          </button>
        </>
      )}
      {stats.toCheck.length > 0 && (
        <>
          <span className="statusbar__dot" />
          <button
            className="statusbar__check"
            onClick={() => {
              const now = usePlayhead.getState().ms;
              const next = stats.toCheck.find((c) => c.startMs > now + 50) ?? stats.toCheck[0];
              select(next.id);
              usePlayhead.getState().seek(next.startMs);
            }}
            title="Jump to the next caption with words or timing to check"
          >
            {stats.toCheck.length} to check · next
          </button>
        </>
      )}
      {stats.overlaps.size === 0 && problems.length === 0 && stats.toCheck.length === 0 && stats.count > 0 && (
        <span className="statusbar__ok">Ready to export</span>
      )}
    </footer>
  );
}

/** Editor keyboard shortcuts. Text fields keep their normal keys. */
function useShortcuts() {
  const startExport = useExport((s) => s.start);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      if (st.view !== "workspace" || st.job) return;
      // Key events can come from the document itself (nothing focused), not only elements.
      const target = e.target instanceof Element ? e.target : null;
      const typing = target?.closest("input, textarea, [contenteditable]") != null;
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();
      if (document.querySelector(".modal, .overlay")) return;

      if (mod && key === "z") {
        e.preventDefault();
        if (e.shiftKey) st.redo();
        else st.undo();
      } else if (mod && key === "y") {
        e.preventDefault();
        st.redo();
      } else if (mod && key === "s") {
        e.preventDefault();
        void saveProject(e.shiftKey);
      } else if (mod && key === "e") {
        e.preventDefault();
        startExport();
      } else if (mod && key === "b" && !e.shiftKey && st.history.present.find((t) => t.id === st.activeTrackId)?.language === "bn") {
        e.preventDefault();
        useBanglaTyping.getState().toggle();
        const on = useBanglaTyping.getState().on;
        toast(on ? "Bangla typing on" : "Bangla typing off", { description: on ? "English letters become Bangla: ami → আমি" : "Letters stay as you type them." });
      } else if (mod && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        e.preventDefault();
        const track = st.history.present.find((t) => t.id === st.activeTrackId);
        if (!track) return;
        const sorted = sortCues(track.cues);
        const now = usePlayhead.getState().ms;
        const next = e.key === "ArrowDown" ? sorted.find((c) => c.startMs > now + 30) : [...sorted].reverse().find((c) => c.startMs < now - 300);
        if (next) {
          usePlayhead.getState().seek(next.startMs);
          st.selectCue(next.id);
        }
      } else if (!typing && e.key === " ") {
        e.preventDefault();
        usePlayhead.getState().togglePlay();
      } else if (!typing && (e.key === "Delete" || e.key === "Backspace") && st.selectedCueId) {
        e.preventDefault();
        st.deleteCue(st.selectedCueId);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [startExport]);
}
