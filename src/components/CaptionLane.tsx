// The caption lane: every cue drawn as a block on the video's timeline, like a
// subtitle track in an editor. Click anywhere to jump there.
import { memo, useRef, useState } from "react";
import { usePlayhead } from "../state/playhead";
import { useStore } from "../state/store";
import { overlappingIds } from "../lib/cueOps";
import { formatClock } from "../lib/time";
import type { Cue } from "../lib/types";

export const CaptionLane = memo(function CaptionLane({
  cues,
  durationMs,
  activeId,
  onSeek,
}: {
  cues: Cue[];
  durationMs: number;
  activeId: string | null;
  onSeek: (ms: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const selectCue = useStore((s) => s.selectCue);
  const selectedId = useStore((s) => s.selectedCueId);
  const [hover, setHover] = useState<{ x: number; ms: number } | null>(null);
  const overlaps = overlappingIds(cues);
  const total = Math.max(1, durationMs);

  const msAt = (clientX: number) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.max(0, Math.min(total, ((clientX - r.left) / r.width) * total));
  };

  return (
    <div
      className="lane"
      ref={ref}
      onPointerDown={(e) => onSeek(msAt(e.clientX))}
      onPointerMove={(e) => {
        const r = ref.current!.getBoundingClientRect();
        setHover({ x: e.clientX - r.left, ms: msAt(e.clientX) });
      }}
      onPointerLeave={() => setHover(null)}
      role="presentation"
    >
      <div className="lane__track">
        {cues.map((c) => {
          const left = (c.startMs / total) * 100;
          const width = Math.max(0.15, ((c.endMs - c.startMs) / total) * 100);
          const cls = [
            "lane__cue",
            c.id === activeId && "is-active",
            c.id === selectedId && "is-selected",
            overlaps.has(c.id) && "is-overlap",
            !c.text.trim() && "is-empty",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <span
              key={c.id}
              className={cls}
              style={{ left: `${left}%`, width: `${width}%` }}
              onPointerDown={(e) => {
                e.stopPropagation();
                selectCue(c.id);
                onSeek(c.startMs);
              }}
              title={c.text}
            />
          );
        })}
      </div>
      <LanePlayhead total={total} />
      {hover && (
        <span className="lane__hover mono" style={{ left: hover.x }}>
          {formatClock(hover.ms)}
        </span>
      )}
    </div>
  );
});

function LanePlayhead({ total }: { total: number }) {
  const ms = usePlayhead((s) => s.ms);
  return <span className="lane__playhead" style={{ left: `${Math.min(100, (ms / total) * 100)}%` }} aria-hidden />;
}
