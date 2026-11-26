// "Get a video from a link": paste a link, choose the part (on a strip of the video's
// own preview frames) and the quality, and CaptionNest fetches it and opens it for
// captions. State lives in state/link.ts.
import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowsClockwise, DownloadSimple, FolderSimple, Info, LinkSimple, WarningCircle } from "@phosphor-icons/react";
import { api } from "../lib/api";
import { describeError } from "../lib/errors";
import { formatClock, formatDuration, parseTimecode } from "../lib/time";
import { frameAt } from "../lib/links";
import type { LinkQuality, QualityChoice, Storyboard } from "../lib/types";
import { Button, Modal, ProgressBar, formatBytes } from "../components/ui";
import { useStore } from "../state/store";
import { LINK_TOOLS_ID, choiceOf, estimateOf, folderOf, useLink } from "../state/link";

// ---------- pictures from the site (fetched by Rust; the page has no internet) ----------

const images = new Map<string, Promise<string>>();

function loadImage(url: string): Promise<string> {
  let p = images.get(url);
  if (!p) {
    p = api.linkImage(url).then((buf) => URL.createObjectURL(new Blob([buf])));
    images.set(url, p);
    p.catch(() => images.delete(url));
  }
  return p;
}

function useImage(url: string | null | undefined): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setSrc(null);
    if (url) loadImage(url).then((s) => live && setSrc(s), () => {});
    return () => {
      live = false;
    };
  }, [url]);
  return src;
}

function Frame({ sb, ms, className = "" }: { sb: Storyboard; ms: number; className?: string }) {
  const f = frameAt(sb, ms);
  const src = useImage(f.url);
  const pos = (i: number, n: number) => (n > 1 ? (i / (n - 1)) * 100 : 0);
  return (
    <span
      className={`frame ${src ? "is-loaded" : ""} ${className}`}
      style={
        src
          ? {
              backgroundImage: `url(${src})`,
              backgroundSize: `${sb.columns * 100}% ${sb.rows * 100}%`,
              backgroundPosition: `${pos(f.col, sb.columns)}% ${pos(f.row, sb.rows)}%`,
            }
          : undefined
      }
    />
  );
}

// ---------- time fields ----------

const fmt = (ms: number) => {
  const tenths = Math.floor((ms % 1000) / 100);
  return formatClock(ms) + (tenths ? `.${tenths}` : "");
};

function TimeField({ label, value, onCommit, id }: { label: string; value: number; onCommit: (ms: number) => boolean; id: string }) {
  const [text, setText] = useState(fmt(value));
  const [bad, setBad] = useState(false);
  useEffect(() => {
    setText(fmt(value));
    setBad(false);
  }, [value]);
  const commit = () => {
    const ms = parseTimecode(text);
    const ok = ms != null && onCommit(ms);
    setBad(!ok);
    if (!ok) setText(fmt(value));
  };
  return (
    <label className="trim__field" htmlFor={id}>
      <span className="label">{label}</span>
      <input
        id={id}
        className={`field mono ${bad ? "is-bad" : ""}`}
        value={text}
        inputMode="decimal"
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          }
        }}
      />
    </label>
  );
}

// ---------- the trim strip ----------

const MIN_PART_MS = 1000;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function TrimStrip({ durationMs, fromMs, toMs, storyboard, onChange }: { durationMs: number; fromMs: number; toMs: number; storyboard: Storyboard | null; onChange: (from: number, to: number) => void }) {
  const strip = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(560);
  const [drag, setDrag] = useState<null | { kind: "from" | "to" | "move"; x0: number; from0: number; to0: number }>(null);
  const [peek, setPeek] = useState<number | null>(null);

  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Tiles keep the frames' 16:9 shape at the strip's 58 px height.
  const tiles = Math.max(4, Math.round(width / 103));
  const pct = (ms: number) => (durationMs ? (ms / durationMs) * 100 : 0);
  const msAt = (clientX: number) => {
    const r = strip.current!.getBoundingClientRect();
    return clamp(((clientX - r.left) / r.width) * durationMs, 0, durationMs);
  };

  useEffect(() => {
    if (!drag) return;
    const move = (e: PointerEvent) => {
      const r = strip.current!.getBoundingClientRect();
      const dms = ((e.clientX - drag.x0) / r.width) * durationMs;
      if (drag.kind === "from") {
        const f = clamp(drag.from0 + dms, 0, drag.to0 - MIN_PART_MS);
        onChange(f, drag.to0);
        setPeek(f);
      } else if (drag.kind === "to") {
        const t = clamp(drag.to0 + dms, drag.from0 + MIN_PART_MS, durationMs);
        onChange(drag.from0, t);
        setPeek(t);
      } else {
        const len = drag.to0 - drag.from0;
        const f = clamp(drag.from0 + dms, 0, durationMs - len);
        onChange(f, f + len);
        setPeek(f);
      }
    };
    const up = () => {
      setDrag(null);
      setPeek(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [drag, durationMs, onChange]);

  const begin = (kind: "from" | "to" | "move") => (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDrag({ kind, x0: e.clientX, from0: fromMs, to0: toMs });
    setPeek(kind === "to" ? toMs : fromMs);
  };

  // A click on the strip moves the nearer edge there.
  const jump = (e: React.PointerEvent) => {
    const ms = msAt(e.clientX);
    if (Math.abs(ms - fromMs) <= Math.abs(ms - toMs)) {
      const f = clamp(ms, 0, toMs - MIN_PART_MS);
      onChange(f, toMs);
      setDrag({ kind: "from", x0: e.clientX, from0: f, to0: toMs });
      setPeek(f);
    } else {
      const t = clamp(ms, fromMs + MIN_PART_MS, durationMs);
      onChange(fromMs, t);
      setDrag({ kind: "to", x0: e.clientX, from0: fromMs, to0: t });
      setPeek(t);
    }
  };

  const keys = (edge: "from" | "to") => (e: React.KeyboardEvent) => {
    const step = e.shiftKey ? 10_000 : e.key === "PageUp" || e.key === "PageDown" ? 60_000 : 1000;
    const dir = e.key === "ArrowRight" || e.key === "ArrowUp" || e.key === "PageUp" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowDown" || e.key === "PageDown" ? -1 : 0;
    let target: number | null = dir ? (edge === "from" ? fromMs : toMs) + dir * step : null;
    if (e.key === "Home") target = edge === "from" ? 0 : fromMs + MIN_PART_MS;
    if (e.key === "End") target = edge === "from" ? toMs - MIN_PART_MS : durationMs;
    if (target == null) return;
    e.preventDefault();
    if (edge === "from") onChange(clamp(target, 0, toMs - MIN_PART_MS), toMs);
    else onChange(fromMs, clamp(target, fromMs + MIN_PART_MS, durationMs));
  };

  return (
    <div className={`trim ${drag ? "is-dragging" : ""}`}>
      <div className="trim__strip" ref={strip} onPointerDown={jump}>
        <div className="trim__frames" aria-hidden>
          {Array.from({ length: tiles }, (_, i) => {
            const ms = ((i + 0.5) / tiles) * durationMs;
            return storyboard ? <Frame key={i} sb={storyboard} ms={ms} /> : <span key={i} className="frame frame--blank" />;
          })}
        </div>
        <div className="trim__shade" style={{ left: 0, width: `${pct(fromMs)}%` }} aria-hidden />
        <div className="trim__shade" style={{ left: `${pct(toMs)}%`, right: 0 }} aria-hidden />
        <div className="trim__sel" style={{ left: `${pct(fromMs)}%`, width: `${pct(toMs - fromMs)}%` }} onPointerDown={begin("move")}>
          <button
            type="button"
            className="trim__handle trim__handle--from"
            role="slider"
            aria-label="Start of the part"
            aria-valuemin={0}
            aria-valuemax={Math.round(durationMs / 1000)}
            aria-valuenow={Math.round(fromMs / 1000)}
            aria-valuetext={fmt(fromMs)}
            onPointerDown={begin("from")}
            onKeyDown={keys("from")}
          />
          <button
            type="button"
            className="trim__handle trim__handle--to"
            role="slider"
            aria-label="End of the part"
            aria-valuemin={0}
            aria-valuemax={Math.round(durationMs / 1000)}
            aria-valuenow={Math.round(toMs / 1000)}
            aria-valuetext={fmt(toMs)}
            onPointerDown={begin("to")}
            onKeyDown={keys("to")}
          />
        </div>
        {peek != null && (
          <div className="trim__peek" style={{ left: `${clamp(pct(peek), 8, 92)}%` }} aria-hidden>
            {storyboard && <Frame sb={storyboard} ms={peek} className="frame--large" />}
            <span className="trim__peektime mono">{fmt(peek)}</span>
          </div>
        )}
      </div>
      <div className="trim__scale mono" aria-hidden>
        <span>0:00</span>
        <span>{formatClock(durationMs)}</span>
      </div>
      <div className="trim__fields">
        <TimeField id="link-from" label="Start" value={fromMs} onCommit={(ms) => ms >= 0 && ms <= toMs - MIN_PART_MS && (onChange(ms, toMs), true)} />
        <TimeField id="link-to" label="End" value={toMs} onCommit={(ms) => ms <= durationMs && ms >= fromMs + MIN_PART_MS && (onChange(fromMs, ms), true)} />
        <div className="trim__len">
          <span className="label">Length</span>
          <span className="trim__lenval">{formatDuration(toMs - fromMs)}</span>
        </div>
      </div>
    </div>
  );
}

// ---------- quality ----------

const QUALITY_TEXT: Record<LinkQuality, { title: string; detail: (c: QualityChoice) => string }> = {
  editing: {
    title: "For editing",
    // Only H.264 opens everywhere; say so only when that's what the site has.
    detail: (c) =>
      !c.vcodec || c.vcodec.startsWith("avc")
        ? `MP4${c.height ? ` · up to ${c.height}p` : ""} · opens in CapCut and every editor`
        : `${(c.ext ?? "video").toUpperCase()}${c.height ? ` · ${c.height}p` : ""} · this site has no MP4 version`,
  },
  best: { title: "Highest quality", detail: (c) => `${c.height ? `${c.height}p` : "Best available"}${c.ext ? ` · ${c.ext.toUpperCase()}` : ""} · some editors can't open it` },
  audio: { title: "Sound only", detail: (c) => `${(c.ext ?? "audio").toUpperCase()} · smallest, and all that captions need` },
};

function QualityPicker() {
  const link = useLink();
  const info = link.info!;
  const seconds = (link.toMs - link.fromMs) / 1000;
  return (
    <fieldset className="tracks-pick linkq">
      <legend className="label">Quality</legend>
      {info.choices.map((c) => {
        const size = link.whole ? c.wholeBytes : c.partBytesPerSec ? c.partBytesPerSec * seconds : null;
        const on = link.quality === c.quality;
        return (
          <label key={c.quality} className={`radio-card ${on ? "is-checked" : ""} ${c.available ? "" : "is-disabled"}`}>
            <input type="radio" name="link-quality" checked={on} disabled={!c.available} onChange={() => link.setQuality(c.quality)} />
            <span className="radio-card__title">
              {QUALITY_TEXT[c.quality].title}
              {size ? <span className="linkq__size mono">about {formatBytes(size)}</span> : null}
            </span>
            <span className="radio-card__meta">{c.available ? QUALITY_TEXT[c.quality].detail(c) : "Not offered for this video"}</span>
          </label>
        );
      })}
    </fieldset>
  );
}

// ---------- panels ----------

function LinkField({ autoFocus }: { autoFocus?: boolean }) {
  const url = useLink((s) => s.url);
  const phase = useLink((s) => s.phase);
  const setUrl = useLink((s) => s.setUrl);
  const read = useLink((s) => s.read);
  const reading = phase === "reading";
  return (
    <form
      className="linkfield"
      onSubmit={(e) => {
        e.preventDefault();
        void read();
      }}
    >
      <LinkSimple size={18} className="linkfield__icon" aria-hidden />
      <input
        className="linkfield__input"
        value={url}
        autoFocus={autoFocus}
        placeholder="https://www.youtube.com/watch?v=…"
        aria-label="Video link"
        spellCheck={false}
        autoComplete="off"
        disabled={reading}
        onChange={(e) => setUrl(e.target.value)}
        onPaste={(e) => {
          const text = e.clipboardData.getData("text");
          if (text.trim()) {
            e.preventDefault();
            void read(text);
          }
        }}
      />
      <Button type="submit" size="sm" variant="secondary" disabled={!url.trim()} busy={reading}>
        {reading ? "Reading…" : "Read link"}
      </Button>
    </form>
  );
}

function SetupPanel() {
  const tools = useLink((s) => s.tools);
  const dl = useStore((s) => s.downloads[LINK_TOOLS_ID]);
  const afterSetup = useLink((s) => s.afterSetup);
  const working = dl && (dl.state === "downloading" || dl.state === "installing" || dl.state === "verifying");
  useEffect(() => {
    if (dl?.state === "done") void afterSetup();
  }, [dl?.state, afterSetup]);
  return (
    <div className="linksetup">
      <p>
        To get videos from links, CaptionNest uses two free programs: <strong>yt-dlp</strong>, which reads video sites, and <strong>Deno</strong>, which yt-dlp
        needs for YouTube. They're downloaded once from their official pages (about {formatBytes(tools?.downloadBytes ?? 95_000_000)}), checked, and kept up to
        date.
      </p>
      {working && (
        <div className="linksetup__progress">
          <ProgressBar value={dl.total ? dl.received / dl.total : null} label="Downloading the video-link programs" />
          <span className="hint mono">
            {dl.state === "installing" ? "Setting up…" : `${formatBytes(dl.received)} of ${formatBytes(dl.total)}`}
          </span>
        </div>
      )}
      {dl?.state === "failed" && dl.error && (
        <p className="dl__error" role="alert">
          <WarningCircle size={16} weight="fill" /> {describeError(dl.error).message}
        </p>
      )}
    </div>
  );
}

function ReadingPanel() {
  const dl = useStore((s) => s.downloads[LINK_TOOLS_ID]);
  const updating = dl && (dl.state === "downloading" || dl.state === "installing");
  return (
    <div className="linkcard linkcard--loading" aria-busy>
      <span className="linkcard__thumb" />
      <div className="linkcard__text">
        <span className="linkcard__line" />
        <span className="linkcard__line linkcard__line--short" />
        <span className="hint">
          {updating ? `Updating the video-link downloader first… ${dl.total ? Math.round((dl.received / dl.total) * 100) : 0}%` : "Reading the link. YouTube takes a few seconds."}
        </span>
      </div>
    </div>
  );
}

function InfoCard() {
  const info = useLink((s) => s.info)!;
  const thumb = useImage(info.thumbnail);
  return (
    <div className="linkcard">
      <span className="linkcard__thumb" style={thumb ? { backgroundImage: `url(${thumb})` } : undefined} />
      <div className="linkcard__text">
        <p className="linkcard__title" title={info.title} dir="auto">
          {info.title}
        </p>
        <p className="linkcard__meta">
          {[info.uploader, info.site, info.durationMs ? formatClock(info.durationMs) : null].filter(Boolean).join(" · ")}
        </p>
      </div>
    </div>
  );
}

function ReadyPanel() {
  const link = useLink();
  const info = link.info!;
  const duration = info.durationMs ?? 0;
  const folder = folderOf(link);
  const home = useMemo(() => folder.replace(/^\/Users\/[^/]+/, "~").replace(/^[A-Z]:\\Users\\[^\\]+/i, "~"), [folder]);
  return (
    <>
      <InfoCard />
      <fieldset className="tracks-pick">
        <legend className="label">What to get</legend>
        <label className={`radio-card ${link.whole ? "is-checked" : ""}`}>
          <input type="radio" name="link-part" checked={link.whole} onChange={() => link.setWhole(true)} />
          <span className="radio-card__title">The whole video</span>
          <span className="radio-card__meta">{duration ? formatDuration(duration) : "Its full length"}</span>
        </label>
        <label className={`radio-card ${!link.whole ? "is-checked" : ""} ${duration ? "" : "is-disabled"}`}>
          <input type="radio" name="link-part" checked={!link.whole} disabled={!duration} onChange={() => link.setWhole(false)} />
          <span className="radio-card__title">Only a part</span>
          <span className="radio-card__meta">Just the stretch you need: faster, and only that part is downloaded</span>
        </label>
      </fieldset>
      {!link.whole && duration > 0 && <TrimStrip durationMs={duration} fromMs={link.fromMs} toMs={link.toMs} storyboard={info.storyboard} onChange={link.setPart} />}
      <QualityPicker />
      <div className="linkfolder">
        <FolderSimple size={18} aria-hidden />
        <span className="linkfolder__text">
          <span className="label">Save to</span>
          <span className="linkfolder__path mono" title={folder}>
            {home}
          </span>
        </span>
        <Button size="sm" variant="ghost" onClick={() => void link.chooseFolder()}>
          Change…
        </Button>
      </div>
      <p className="linknote">
        <Info size={15} aria-hidden /> Only get videos you made or have permission to use. Downloading other people's videos can break YouTube's rules and
        copyright law.
      </p>
    </>
  );
}

function FetchPanel() {
  const link = useLink();
  const info = link.info;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const elapsed = now - link.startedAt;
  const left = link.progress && link.progress > 0.05 && link.stage === "downloading" ? (elapsed / link.progress) * (1 - link.progress) : null;
  return (
    <div className="linkfetch">
      {info && <InfoCard />}
      <div className="linkfetch__body">
        <p className="linkfetch__what">
          {link.stage === "finishing"
            ? "Putting the picture and sound together…"
            : link.whole
              ? "Getting the whole video…"
              : `Getting ${formatDuration(link.toMs - link.fromMs)} from ${fmt(link.fromMs)}…`}
        </p>
        <ProgressBar value={link.stage === "finishing" ? null : link.progress} label="Download progress" />
        <p className="hint mono linkfetch__nums">
          {formatBytes(link.received)}
          {link.estimate ? ` of about ${formatBytes(link.estimate)}` : ""}
          {link.speed > 0 ? ` · ${formatBytes(link.speed)}/s` : ""}
          {left != null ? ` · about ${formatDuration(left)} left` : ""}
        </p>
      </div>
    </div>
  );
}

function FailedPanel() {
  const error = useLink((s) => s.error);
  const failedAt = useLink((s) => s.failedAt);
  const [details, setDetails] = useState(false);
  if (!error) return null;
  const d = describeError(error);
  return (
    <div className="linkfail" role="alert">
      <WarningCircle size={22} weight="fill" className="linkfail__icon" />
      <div>
        <p className="linkfail__title">{failedAt === "read" ? "Couldn't read this link" : d.title}</p>
        <p className="linkfail__msg">{d.message}</p>
        {error.detail && (
          <>
            <button type="button" className="linkbtn" onClick={() => setDetails(!details)}>
              {details ? "Hide details" : "Show details"}
            </button>
            {details && <pre className="linkfail__detail selectable">{error.detail}</pre>}
          </>
        )}
      </div>
    </div>
  );
}

export function LinkDialog() {
  const link = useLink();
  const dl = useStore((s) => s.downloads[LINK_TOOLS_ID]);
  const open = link.phase !== "closed" && !link.hidden;
  const settingUp = dl && (dl.state === "downloading" || dl.state === "installing" || dl.state === "verifying");
  const estimate = link.info ? estimateOf(link) : null;
  const validPart = link.whole || link.toMs - link.fromMs >= MIN_PART_MS;
  const choice = choiceOf(link);

  let footer: React.ReactNode;
  switch (link.phase) {
    case "setup":
      footer = (
        <>
          <Button variant="ghost" onClick={link.close}>
            Cancel
          </Button>
          <Button variant="primary" icon={<DownloadSimple size={16} weight="bold" />} busy={!!settingUp} onClick={() => void link.setup()}>
            {settingUp ? "Downloading…" : "Download and continue"}
          </Button>
        </>
      );
      break;
    case "ready":
      footer = (
        <>
          <Button variant="ghost" onClick={link.close}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={<DownloadSimple size={16} weight="bold" />}
            disabled={!validPart || !choice?.available || !folderOf(link)}
            onClick={() => void link.start()}
          >
            {link.whole ? "Get video" : "Get this part"}
            {estimate ? <span className="btn__aside">{formatBytes(estimate)}</span> : null}
          </Button>
        </>
      );
      break;
    case "fetching":
      footer = (
        <Button variant="secondary" onClick={() => void link.cancel()}>
          Stop
        </Button>
      );
      break;
    case "failed":
      footer = (
        <>
          <Button variant="ghost" onClick={link.close}>
            Close
          </Button>
          <Button variant="primary" icon={<ArrowsClockwise size={16} />} onClick={link.retry}>
            Try again
          </Button>
        </>
      );
      break;
    default:
      footer = (
        <Button variant="ghost" onClick={link.close}>
          Cancel
        </Button>
      );
  }

  return (
    <Modal
      open={open}
      onOpenChange={(o) => !o && link.close()}
      title={link.phase === "setup" ? "Set up video links" : "Get a video from a link"}
      description={
        link.phase === "setup"
          ? "A one-time download, then paste links from YouTube and other video sites."
          : "Paste a link from YouTube or another video site. CaptionNest saves the video (or just the part you need) and opens it for captions."
      }
      size="lg"
      className="linkdialog"
      footer={footer}
    >
      {link.phase === "setup" && <SetupPanel />}
      {(link.phase === "idle" || link.phase === "reading" || link.phase === "ready" || (link.phase === "failed" && link.failedAt === "read")) && (
        <LinkField autoFocus={link.phase === "idle"} />
      )}
      {link.phase === "reading" && <ReadingPanel />}
      {link.phase === "ready" && <ReadyPanel />}
      {link.phase === "fetching" && <FetchPanel />}
      {link.phase === "failed" && <FailedPanel />}
    </Modal>
  );
}
