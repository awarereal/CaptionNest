import { useEffect, useState } from "react";
import { create } from "zustand";
import { pickSave } from "../lib/fileFlows";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CheckCircle, WarningCircle, Warning, FileText, FolderOpen, Question, MagicWand } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { usePlayhead } from "../state/playhead";
import { api, toAppError } from "../lib/api";
import { languageName } from "../lib/languages";
import { describeError } from "../lib/errors";
import type { ValidationIssue, ValidationReport } from "../lib/types";
import { Button, Modal } from "../components/ui";

interface ExportState {
  open: boolean;
  start: () => void;
  close: () => void;
}

export const useExport = create<ExportState>((set) => ({
  open: false,
  start: () => {
    const st = useStore.getState();
    if (st.view === "workspace" && st.project && !st.job) set({ open: true });
  },
  close: () => set({ open: false }),
}));

const ISSUE_TEXT: Record<ValidationIssue["kind"], (n: number) => string> = {
  empty_text: (n) => `${n} ${n === 1 ? "caption has" : "captions have"} no text`,
  negative_time: (n) => `${n} ${n === 1 ? "caption starts" : "captions start"} before 0:00`,
  invalid_duration: (n) => `${n} ${n === 1 ? "caption ends" : "captions end"} before ${n === 1 ? "it starts" : "they start"}`,
  out_of_order: (n) => `${n} ${n === 1 ? "caption is" : "captions are"} out of order (fixed automatically on export)`,
  overlap: (n) => `${n} ${n === 1 ? "caption overlaps" : "captions overlap"} the next one`,
};

export function ExportDialog() {
  const open = useExport((s) => s.open);
  const close = useExport((s) => s.close);
  const tracks = useStore((s) => s.history.present);
  const activeId = useStore((s) => s.activeTrackId);
  const project = useStore((s) => s.project);
  const [trackId, setTrackId] = useState<string | null>(null);
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<{ path: string; cueCount: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const track = tracks.find((t) => t.id === trackId) ?? null;

  useEffect(() => {
    if (open) {
      setTrackId(activeId);
      setDone(null);
      setError(null);
    }
  }, [open, activeId]);

  useEffect(() => {
    if (!open || !track) return;
    let alive = true;
    api.validateCues(track.cues).then((r) => alive && setReport(r));
    return () => {
      alive = false;
    };
  }, [open, track]);

  if (!project) return null;

  const issues = report?.issues ?? [];
  const blocking = issues.filter((i) => i.blocking);
  const overlaps = issues.filter((i) => i.kind === "overlap");
  const grouped = Object.entries(
    issues.reduce<Record<string, ValidationIssue[]>>((acc, i) => {
      (acc[i.kind] ??= []).push(i);
      return acc;
    }, {}),
  ) as [ValidationIssue["kind"], ValidationIssue[]][];

  const fileName = `${project.video.fileName.replace(/\.[^.]+$/, "")} - ${languageName(track?.language, "Captions")}.srt`;

  const show = (issue: ValidationIssue) => {
    const st = useStore.getState();
    if (trackId) st.setActiveTrack(trackId);
    st.selectCue(issue.cueId);
    const cue = track?.cues.find((c) => c.id === issue.cueId);
    if (cue) usePlayhead.getState().seek(cue.startMs);
    close();
  };

  const fixOverlaps = async () => {
    if (!track) return;
    const st = useStore.getState();
    if (st.activeTrackId !== track.id) st.setActiveTrack(track.id);
    st.replaceCues(await api.fixOverlaps(track.cues));
  };

  const doExport = async (fixFirst: boolean) => {
    if (!track) return;
    setError(null);
    const picked = await pickSave({
      title: "Export SRT for CapCut",
      defaultPath: fileName,
      filters: [{ name: "SubRip subtitles", extensions: ["srt"] }],
    });
    if (!picked) return;
    const path = picked.toLowerCase().endsWith(".srt") ? picked : `${picked}.srt`;
    setBusy(true);
    try {
      let cues = track.cues;
      if (fixFirst) {
        cues = await api.fixOverlaps(cues);
        await fixOverlaps();
      }
      // Always export the latest edited text and timing.
      const res = await api.exportSrt(path, cues);
      setDone({ path: res.path, cueCount: res.cueCount });
    } catch (e) {
      setError(describeError(toAppError(e)).message);
    } finally {
      setBusy(false);
    }
  };

  if (done) {
    return (
      <Modal
        open={open}
        onOpenChange={(o) => !o && close()}
        title="SRT exported"
        size="md"
        footer={
          <>
            <Button variant="ghost" icon={<Question size={16} />} onClick={() => (close(), useStore.getState().openPanel("help"))}>
              How to import into CapCut
            </Button>
            <span className="spacer" />
            <Button variant="secondary" icon={<FolderOpen size={16} />} onClick={() => void revealItemInDir(done.path)}>
              Show in folder
            </Button>
            <Button variant="primary" onClick={close}>
              Done
            </Button>
          </>
        }
      >
        <div className="export-done">
          <CheckCircle size={40} weight="fill" />
          <p className="export-done__name selectable">{done.path.split(/[\\/]/).pop()}</p>
          <p className="hint">
            {done.cueCount} captions · UTF-8 · ready for CapCut Desktop and Web. In CapCut, import your original video and this file; each caption
            becomes its own editable clip.
          </p>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open={open}
      onOpenChange={(o) => !o && close()}
      title="Export SRT for CapCut"
      description="CaptionNest checks the captions, then saves a subtitle file with their text and timing. Fonts and styling are set inside CapCut."
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <span className="spacer" />
          {overlaps.length > 0 && blocking.length === 0 && (
            <Button variant="secondary" onClick={() => void doExport(false)} disabled={busy}>
              Export as is
            </Button>
          )}
          <Button
            variant="primary"
            icon={overlaps.length > 0 ? <MagicWand size={16} /> : <FileText size={16} weight="bold" />}
            busy={busy}
            disabled={!report || blocking.length > 0 || !track || track.cues.length === 0}
            onClick={() => void doExport(overlaps.length > 0)}
          >
            {overlaps.length > 0 ? "Fix overlaps and export" : "Export SRT…"}
          </Button>
        </>
      }
    >
      {tracks.length > 1 && (
        <fieldset className="tracks-pick">
          <legend className="label">Captions to export</legend>
          {tracks.map((t) => (
            <label key={t.id} className={`radio-card ${t.id === trackId ? "is-checked" : ""}`}>
              <input type="radio" name="export-track" checked={t.id === trackId} onChange={() => setTrackId(t.id)} />
              <span className="radio-card__title">{languageName(t.language)}</span>
              <span className="radio-card__meta">
                {t.kind === "original" ? "Original transcript" : "Translation"} · {t.cues.length} captions
              </span>
            </label>
          ))}
        </fieldset>
      )}

      <ul className="checks" aria-live="polite">
        {!report ? (
          <li className="check">
            <span className="spinner" /> Checking captions…
          </li>
        ) : issues.length === 0 ? (
          <li className="check check--ok">
            <CheckCircle size={18} weight="fill" /> {report.cueCount} captions, all with valid timing and text
          </li>
        ) : (
          grouped.map(([kind, list]) => (
            <li key={kind} className={`check ${list[0].blocking ? "check--bad" : "check--warn"}`}>
              {list[0].blocking ? <WarningCircle size={18} weight="fill" /> : <Warning size={18} weight="fill" />}
              <span className="check__text">{ISSUE_TEXT[kind](kind === "overlap" ? list.length : list.length)}</span>
              {kind === "overlap" ? (
                <>
                  <button className="btn btn--quiet" onClick={() => show(list[0])}>
                    Show
                  </button>
                  <button className="btn btn--quiet" onClick={() => void fixOverlaps()}>
                    Fix now
                  </button>
                </>
              ) : kind !== "out_of_order" ? (
                <button className="btn btn--quiet" onClick={() => show(list[0])}>
                  Show first (#{list[0].position})
                </button>
              ) : null}
            </li>
          ))
        )}
      </ul>

      {blocking.length > 0 && <p className="hint">Fix the captions marked in red, then export.</p>}
      {track && track.cues.some((c) => (c.check ?? []).some((w) => w && c.text.includes(w))) && (
        <p className="hint">
          Some captions still have words marked to check. You can export now, or review them first with “to check · next” in the
          caption list.
        </p>
      )}
      {overlaps.length > 0 && blocking.length === 0 && (
        <p className="hint">
          Overlapping captions may stack on screen in CapCut. “Fix overlaps” ends each caption where the next one begins.
        </p>
      )}

      <div className="export-file">
        <FileText size={20} />
        <div>
          <p className="export-file__name">{fileName}</p>
          <p className="hint">UTF-8 SubRip (.srt) · numbered cues · HH:MM:SS,mmm timestamps</p>
        </div>
      </div>
      {error && (
        <p className="dl__error" role="alert">
          {error}
        </p>
      )}
    </Modal>
  );
}
