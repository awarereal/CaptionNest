import { CheckCircle, DownloadSimple, Stop, Trash, ArrowClockwise, Translate, Prohibit, Waveform } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { MODEL_INFO } from "../lib/models";
import type { WhisperModelEntry } from "../lib/types";
import { Badge, Button, Meter, ProgressBar, formatBytes } from "./ui";
import { describeError } from "../lib/errors";

export function formatEta(remainingBytes: number, bytesPerSec: number) {
  if (!bytesPerSec || bytesPerSec < 1) return "";
  const s = Math.round(remainingBytes / bytesPerSec);
  if (s < 60) return `${s} s left`;
  const m = Math.round(s / 60);
  return m < 60 ? `${m} min left` : `${Math.floor(m / 60)} h ${m % 60} min left`;
}

export function DownloadStatus({ id, total, partial = 0 }: { id: string; total: number; partial?: number }) {
  const d = useStore((s) => s.downloads[id]);
  const cancel = useStore((s) => s.cancelDownload);
  if (!d || d.state === "done" || d.state === "cancelled") return null;
  if (d.state === "failed") {
    const err = d.error ? describeError(d.error) : null;
    return (
      <p className="dl__error" role="alert">
        {err?.message ?? "The download failed."}
      </p>
    );
  }
  const received = d.received || partial;
  const size = d.total || total;
  const value = d.state === "installing" || d.state === "verifying" ? null : size ? received / size : null;
  return (
    <div className="dl" aria-live="polite">
      <ProgressBar value={value} label="Download progress" />
      <div className="dl__row">
        <span className="mono">
          {d.state === "installing" ? "Installing…" : d.state === "verifying" ? "Checking file…" : `${formatBytes(received)} of ${formatBytes(size)}`}
        </span>
        <span className="dl__eta">{d.state === "downloading" ? formatEta(size - received, d.bytesPerSec) : ""}</span>
        <Button variant="ghost" size="sm" icon={<Stop size={14} weight="fill" />} onClick={() => void cancel(id)}>
          Stop
        </Button>
      </div>
    </div>
  );
}

export function ModelCard({
  entry,
  recommended,
  selected,
  onUse,
  compact,
}: {
  entry: WhisperModelEntry;
  recommended?: boolean;
  selected?: boolean;
  onUse?: () => void;
  compact?: boolean;
}) {
  const info = MODEL_INFO[entry.id];
  const download = useStore((s) => s.download);
  const remove = useStore((s) => s.removeModel);
  const dl = useStore((s) => s.downloads[entry.id]);
  const active = dl && (dl.state === "downloading" || dl.state === "installing" || dl.state === "verifying");
  const failed = dl?.state === "failed";
  const resumable = !entry.installed && entry.partialBytes > 0 && !active;

  return (
    <article className={`model ${selected ? "is-selected" : ""} ${entry.installed ? "is-installed" : ""} ${compact ? "model--compact" : ""}`}>
      <header className="model__head">
        <h3 className="model__name display">{info?.name ?? entry.id}</h3>
        {recommended && <Badge tone="gold">{entry.language ? "Best for Bangla" : "Recommended"}</Badge>}
        {entry.language === "bn" && <Badge tone="teal">For Bangla</Badge>}
        {entry.installed && selected && !entry.language && <Badge tone="teal">In use</Badge>}
      </header>
      <p className="model__summary">{info?.summary}</p>
      {info && (
        <div className="model__meters">
          <Meter value={info.accuracy} label="Accuracy" />
          <Meter value={info.speed} label="Speed" />
        </div>
      )}
      <ul className="model__facts">
        <li>
          {entry.language ? <Waveform size={15} /> : info?.translates ? <Translate size={15} /> : <Prohibit size={15} />}
          {entry.language === "bn"
            ? "Used automatically for Bangla speech"
            : info?.translates
              ? "Can make English captions from any language"
              : "Captions in the spoken language only"}
        </li>
        <li className="mono">
          {formatBytes(entry.sizeBytes)} download · {formatBytes(entry.sizeBytes)} on disk
        </li>
      </ul>
      <footer className="model__foot">
        {entry.installed ? (
          <>
            <span className="model__ready">
              <CheckCircle size={17} weight="fill" /> Ready offline
            </span>
            <span className="spacer" />
            {onUse && !selected && !entry.language && (
              <Button size="sm" variant="secondary" onClick={onUse}>
                Use this model
              </Button>
            )}
            {!compact && (
              <Button size="sm" variant="ghost" icon={<Trash size={15} />} onClick={() => void remove(entry.id)} aria-label={`Remove ${info?.name ?? entry.id}`}>
                Remove
              </Button>
            )}
          </>
        ) : active ? (
          <DownloadStatus id={entry.id} total={entry.sizeBytes} partial={entry.partialBytes} />
        ) : (
          <>
            {failed && <DownloadStatus id={entry.id} total={entry.sizeBytes} />}
            <Button
              variant={recommended && !entry.language ? "primary" : "secondary"}
              icon={failed || resumable ? <ArrowClockwise size={16} /> : <DownloadSimple size={16} weight="bold" />}
              onClick={() => void download(entry.id)}
            >
              {failed ? "Try again" : resumable ? `Resume (${formatBytes(entry.partialBytes)} done)` : `Download ${formatBytes(entry.sizeBytes)}`}
            </Button>
          </>
        )}
      </footer>
    </article>
  );
}
