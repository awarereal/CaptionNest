import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DropdownMenu } from "radix-ui";
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  SpeakerHigh,
  SpeakerSlash,
  Gauge,
  VideoCameraSlash,
  MusicNotes,
  Check,
} from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { usePlayhead } from "../state/playhead";
import { activeIndex, sortCues } from "../lib/cueOps";
import { formatClock } from "../lib/time";
import { isRtl } from "../lib/languages";
import type { Cue } from "../lib/types";
import { Button, IconButton, ProgressBar } from "./ui";
import { CaptionLane } from "./CaptionLane";

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];

export function Player({
  cues = [],
  language,
  showLane = false,
  durationMs,
}: {
  cues?: Cue[];
  language?: string | null;
  showLane?: boolean;
  durationMs: number;
}) {
  const preview = useStore((s) => s.preview);
  const media = useStore((s) => s.media);
  const videoMissing = useStore((s) => s.videoMissing);
  const requestPreview = useStore((s) => s.requestPreview);
  const videoRef = useRef<HTMLVideoElement>(null);
  const [failed, setFailed] = useState(false);
  const [muted, setMuted] = useState(false);
  const [rate, setRate] = useState(1);
  const ms = usePlayhead((s) => s.ms);
  const playing = usePlayhead((s) => s.playing);
  const setPlayhead = usePlayhead((s) => s.set);

  const sorted = useMemo(() => sortCues(cues), [cues]);
  const idx = activeIndex(sorted, ms);
  const active = idx >= 0 ? sorted[idx] : null;
  const rtl = isRtl(language);
  const hasVideo = (media?.video ?? null) !== null && preview.status !== "audio-only";

  useEffect(() => setFailed(false), [preview.url]);

  // Smooth playhead: follow the video every animation frame while playing.
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    const tick = () => {
      const v = videoRef.current;
      if (v) setPlayhead({ ms: v.currentTime * 1000 });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, setPlayhead]);

  const seek = useCallback(
    (target: number) => {
      const v = videoRef.current;
      const clamped = Math.max(0, Math.min(durationMs, target));
      if (v) v.currentTime = clamped / 1000;
      setPlayhead({ ms: clamped });
    },
    [durationMs, setPlayhead],
  );

  const togglePlay = useCallback(() => {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) void v.play().catch(() => setFailed(true));
    else v.pause();
  }, []);

  useEffect(() => {
    usePlayhead.getState().bind({ seek, togglePlay });
  }, [seek, togglePlay]);

  useEffect(() => {
    if (videoRef.current) videoRef.current.playbackRate = rate;
  }, [rate]);

  const jump = (dir: -1 | 1) => {
    if (sorted.length === 0) return;
    const now = usePlayhead.getState().ms;
    const target =
      dir === 1 ? sorted.find((c) => c.startMs > now + 30) : [...sorted].reverse().find((c) => c.startMs < now - 300);
    seek(target ? target.startMs : dir === 1 ? now : 0);
  };

  const unplayable = failed && preview.status === "original";
  const converting = preview.status === "converting";

  return (
    <div className="player">
      <div className={`player__stage ${hasVideo ? "" : "player__stage--audio"}`}>
        {preview.url && !videoMissing && (
          <video
            ref={videoRef}
            className="player__video"
            src={preview.url}
            preload="auto"
            playsInline
            muted={muted}
            onPlay={() => setPlayhead({ playing: true })}
            onPause={() => setPlayhead({ playing: false, ms: (videoRef.current?.currentTime ?? 0) * 1000 })}
            onEnded={() => setPlayhead({ playing: false })}
            onSeeked={() => setPlayhead({ ms: (videoRef.current?.currentTime ?? 0) * 1000 })}
            onError={() => setFailed(true)}
            onClick={togglePlay}
          />
        )}

        {!hasVideo && !unplayable && !converting && !videoMissing && (
          <div className="player__audio" aria-hidden>
            <MusicNotes size={40} />
            <span>{media?.fileName}</span>
          </div>
        )}

        {active && (
          <div className="overlay-cue" aria-live="off">
            <span className="overlay-cue__text caption-text" dir={rtl ? "rtl" : "auto"} lang={language ?? undefined}>
              {active.text}
            </span>
          </div>
        )}

        {(unplayable || videoMissing) && (
          <div className="player__notice">
            <VideoCameraSlash size={34} />
            {videoMissing ? (
              <>
                <p className="player__notice-title">The video isn't where it was</p>
                <p>Your captions are safe. Locate the video to play it again.</p>
              </>
            ) : (
              <>
                <p className="player__notice-title">This video can't play in the preview</p>
                <p>Captions will still be made from it. For playback, CaptionNest can make a preview copy. Your original file isn't changed.</p>
                <Button variant="secondary" onClick={() => void requestPreview()}>
                  Make a preview copy
                </Button>
              </>
            )}
          </div>
        )}

        {converting && (
          <div className="player__notice">
            <p className="player__notice-title">Making a preview copy…</p>
            <div className="player__notice-progress">
              <ProgressBar value={preview.progress ?? null} label="Preview progress" tone="teal" />
            </div>
            <p className="hint">Your original video isn't changed.</p>
          </div>
        )}

        {preview.status === "failed" && (
          <div className="player__notice">
            <VideoCameraSlash size={34} />
            <p className="player__notice-title">A preview couldn't be made</p>
            <p>{preview.error?.message ?? "This video's format isn't supported for preview."} You can still make and edit captions.</p>
          </div>
        )}
      </div>

      <div className="transport">
        <IconButton label={playing ? "Pause" : "Play"} shortcut="Space" onClick={togglePlay} disabled={!preview.url || videoMissing} side="top">
          {playing ? <Pause size={20} weight="fill" /> : <Play size={20} weight="fill" />}
        </IconButton>
        <IconButton label="Previous caption" shortcut="Mod+↑" onClick={() => jump(-1)} disabled={sorted.length === 0} size="sm" side="top">
          <SkipBack size={16} weight="fill" />
        </IconButton>
        <IconButton label="Next caption" shortcut="Mod+↓" onClick={() => jump(1)} disabled={sorted.length === 0} size="sm" side="top">
          <SkipForward size={16} weight="fill" />
        </IconButton>
        <span className="transport__time mono">
          <span className="transport__now">{formatClock(ms)}</span>
          <span className="transport__dur"> / {formatClock(durationMs)}</span>
        </span>
        <Scrubber ms={ms} durationMs={durationMs} onSeek={seek} />
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button className="transport__rate mono" aria-label={`Playback speed ${rate}×`}>
              <Gauge size={15} /> {rate}×
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="menu menu--small" side="top" align="end" sideOffset={6}>
              <div className="menu__viewport">
                {SPEEDS.map((s) => (
                  <DropdownMenu.Item key={s} className="menu__item" onSelect={() => setRate(s)}>
                    <span className="menu__check">{s === rate && <Check size={13} weight="bold" />}</span>
                    <span className="mono">{s}×</span>
                  </DropdownMenu.Item>
                ))}
              </div>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
        <IconButton label={muted ? "Unmute" : "Mute"} onClick={() => setMuted((m) => !m)} size="sm" side="top">
          {muted ? <SpeakerSlash size={17} /> : <SpeakerHigh size={17} />}
        </IconButton>
      </div>

      {showLane && <CaptionLane cues={sorted} durationMs={durationMs} activeId={active?.id ?? null} onSeek={seek} />}
    </div>
  );
}

function Scrubber({ ms, durationMs, onSeek }: { ms: number; durationMs: number; onSeek: (ms: number) => void }) {
  const pct = durationMs > 0 ? Math.min(100, (ms / durationMs) * 100) : 0;
  return (
    <div className="scrubber" style={{ ["--pct" as string]: `${pct}%` }}>
      <input
        type="range"
        min={0}
        max={Math.max(1, durationMs)}
        step={10}
        value={Math.min(ms, durationMs)}
        onChange={(e) => onSeek(Number(e.target.value))}
        aria-label="Position in video"
        aria-valuetext={formatClock(ms)}
      />
    </div>
  );
}
