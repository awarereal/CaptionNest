import { memo, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Play, Scissors, ArrowsMergeIcon, Trash, WarningCircle } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { usePlayhead } from "../state/playhead";
import { activeIndex, overlappingIds, sortCues } from "../lib/cueOps";
import { formatSeconds } from "../lib/time";
import { isRtl } from "../lib/languages";
import type { Cue } from "../lib/types";
import { IconButton } from "./ui";
import { TimeField } from "./TimeField";

export function CueList({ cues, language }: { cues: Cue[]; language: string }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const sorted = useMemo(() => sortCues(cues), [cues]);
  const overlaps = useMemo(() => overlappingIds(sorted), [sorted]);
  const selectedId = useStore((s) => s.selectedCueId);
  const playing = usePlayhead((s) => s.playing);
  const ms = usePlayhead((s) => s.ms);
  const activeIdx = activeIndex(sorted, ms);
  const lastUserScroll = useRef(0);

  const virtualizer = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 84,
    overscan: 10,
    getItemKey: (i) => sorted[i].id,
  });

  // Follow playback, unless the user scrolled the list in the last few seconds.
  useEffect(() => {
    if (!playing || activeIdx < 0) return;
    if (Date.now() - lastUserScroll.current < 3000) return;
    virtualizer.scrollToIndex(activeIdx, { align: "center", behavior: "smooth" });
  }, [activeIdx, playing, virtualizer]);

  // Bring a newly selected cue (e.g. clicked on the lane) into view.
  useLayoutEffect(() => {
    if (!selectedId) return;
    const i = sorted.findIndex((c) => c.id === selectedId);
    if (i >= 0) virtualizer.scrollToIndex(i, { align: "auto" });
  }, [selectedId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (sorted.length === 0) {
    return (
      <div className="cuelist cuelist--empty">
        <p className="display">No captions in this track</p>
        <p className="hint">Use “Add caption” to write one at the current position.</p>
      </div>
    );
  }

  return (
    <div
      ref={parentRef}
      className="cuelist"
      onWheel={() => (lastUserScroll.current = Date.now())}
      onTouchMove={() => (lastUserScroll.current = Date.now())}
      role="list"
      aria-label="Captions"
    >
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((row) => {
          const cue = sorted[row.index];
          return (
            <div
              key={row.key}
              data-index={row.index}
              ref={virtualizer.measureElement}
              className="cuelist__row"
              style={{ transform: `translateY(${row.start}px)` }}
              role="listitem"
            >
              <CueRow
                cue={cue}
                index={row.index}
                active={row.index === activeIdx}
                selected={cue.id === selectedId}
                overlap={overlaps.has(cue.id)}
                isLast={row.index === sorted.length - 1}
                rtl={isRtl(language)}
                language={language}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

const CueRow = memo(function CueRow({
  cue,
  index,
  active,
  selected,
  overlap,
  isLast,
  rtl,
  language,
}: {
  cue: Cue;
  index: number;
  active: boolean;
  selected: boolean;
  overlap: boolean;
  isLast: boolean;
  rtl: boolean;
  language: string;
}) {
  const setText = useStore((s) => s.setCueText);
  const retime = useStore((s) => s.retimeCue);
  const select = useStore((s) => s.selectCue);
  const split = useStore((s) => s.splitCue);
  const merge = useStore((s) => s.mergeCue);
  const remove = useStore((s) => s.deleteCue);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const duration = cue.endMs - cue.startMs;
  const invalid = duration <= 0;
  const empty = !cue.text.trim();

  // Grow the text box with its content.
  useLayoutEffect(() => {
    const el = textRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${el.scrollHeight}px`;
  }, [cue.text]);

  const seekHere = () => {
    const { ms, seek } = usePlayhead.getState();
    if (ms < cue.startMs || ms >= cue.endMs) seek(cue.startMs);
  };

  const cls = ["cue", active && "is-active", selected && "is-selected", overlap && "is-overlap", (empty || invalid) && "is-problem"]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      data-cue-id={cue.id}
      onMouseDown={(e) => {
        select(cue.id);
        if (!(e.target as HTMLElement).closest("input,button")) seekHere();
      }}
      onFocus={() => select(cue.id)}
    >
      <div className="cue__rail">
        <span className="cue__num mono">{index + 1}</span>
      </div>
      <div className="cue__main">
        <div className="cue__times">
          <TimeField label={`Caption ${index + 1} start`} value={cue.startMs} onCommit={(v) => retime(cue.id, v, Math.max(v + 100, cue.endMs))} />
          <span className="cue__arrow" aria-hidden>
            →
          </span>
          <TimeField label={`Caption ${index + 1} end`} value={cue.endMs} invalid={invalid} onCommit={(v) => retime(cue.id, cue.startMs, v)} />
          <span className={`cue__dur mono ${invalid ? "is-bad" : ""}`}>{invalid ? "invalid" : formatSeconds(duration)}</span>
          {overlap && (
            <span className="cue__flag" title="Overlaps a neighboring caption">
              <WarningCircle size={14} weight="fill" /> Overlap
            </span>
          )}
        </div>
        <textarea
          ref={textRef}
          className="cue__text caption-text"
          value={cue.text}
          dir={rtl ? "rtl" : "auto"}
          lang={language}
          rows={1}
          spellCheck={false}
          placeholder="Type the caption…"
          aria-label={`Caption ${index + 1} text`}
          onChange={(e) => setText(cue.id, e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
              e.preventDefault();
              split(cue.id, { caret: e.currentTarget.selectionStart });
            }
          }}
        />
      </div>
      <div className="cue__actions">
        <IconButton label="Play from here" size="sm" side="top" onClick={() => {
          usePlayhead.getState().seek(cue.startMs);
          if (!usePlayhead.getState().playing) usePlayhead.getState().togglePlay();
        }}>
          <Play size={14} weight="fill" />
        </IconButton>
        <IconButton
          label="Split caption"
          shortcut="Mod+Enter"
          size="sm"
          side="top"
          onClick={() => {
            const caret = textRef.current && document.activeElement === textRef.current ? textRef.current.selectionStart : undefined;
            split(cue.id, { caret, atMs: usePlayhead.getState().ms });
          }}
        >
          <Scissors size={15} />
        </IconButton>
        <IconButton label="Merge with next caption" size="sm" side="top" disabled={isLast} onClick={() => merge(cue.id)}>
          <ArrowsMergeIcon size={15} />
        </IconButton>
        <IconButton label="Delete caption" size="sm" side="top" tone="danger" onClick={() => remove(cue.id)}>
          <Trash size={15} />
        </IconButton>
      </div>
    </div>
  );
});
