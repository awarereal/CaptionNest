import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Play, Scissors, ArrowsMergeIcon, Trash, WarningCircle, Eye, Check, Clock } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { usePlayhead } from "../state/playhead";
import { VOICE_LANGUAGES, useVoice } from "../state/voice";
import { activeIndex, overlappingIds, sortCues, wordsToCheck } from "../lib/cueOps";
import { formatSeconds } from "../lib/time";
import { isRtl } from "../lib/languages";
import type { Cue } from "../lib/types";
import { IconButton } from "./ui";
import { TimeField } from "./TimeField";
import { MicButton, PhoneticStrip, VoiceBar } from "./TypingAids";
import { useBanglaTyping, usePhonetic } from "./usePhonetic";

export function CueList({ cues, language }: { cues: Cue[]; language: string }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const phonetic = useBanglaTyping((s) => s.on) && language === "bn";
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
  const items = virtualizer.getVirtualItems();

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
      <div className="cuelist__space" style={{ height: virtualizer.getTotalSize() }}>
        {/* Rows sit in normal flow inside one shifted block, so a caption that grows
            pushes the next one down instead of being drawn over it, even before the
            virtualizer has re-measured it. */}
        <div className="cuelist__rows" style={{ transform: `translateY(${items[0]?.start ?? 0}px)` }}>
        {items.map((row) => {
          const cue = sorted[row.index];
          return (
            <div key={row.key} data-index={row.index} ref={virtualizer.measureElement} className="cuelist__row" role="listitem">
              <CueRow
                cue={cue}
                index={row.index}
                active={row.index === activeIdx}
                selected={cue.id === selectedId}
                overlap={overlaps.has(cue.id)}
                isLast={row.index === sorted.length - 1}
                rtl={isRtl(language)}
                language={language}
                phonetic={phonetic}
              />
            </div>
          );
        })}
        </div>
      </div>
    </div>
  );
}

function fitHeight(el: HTMLTextAreaElement) {
  el.style.height = "0px";
  el.style.height = `${el.scrollHeight}px`;
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
  phonetic,
}: {
  cue: Cue;
  index: number;
  active: boolean;
  selected: boolean;
  overlap: boolean;
  isLast: boolean;
  rtl: boolean;
  language: string;
  /** Latin letters typed into this caption become Bangla. */
  phonetic: boolean;
}) {
  const setText = useStore((s) => s.setCueText);
  const retime = useStore((s) => s.retimeCue);
  const select = useStore((s) => s.selectCue);
  const split = useStore((s) => s.splitCue);
  const merge = useStore((s) => s.mergeCue);
  const remove = useStore((s) => s.deleteCue);
  const markChecked = useStore((s) => s.markChecked);
  const markTimingChecked = useStore((s) => s.markTimingChecked);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const [focused, setFocused] = useState(false);
  const phon = usePhonetic(textRef, phonetic, (text) => setText(cue.id, text));
  const voiceHere = useVoice((s) => s.phase !== "idle" && s.target?.cueId === cue.id);
  const showMic = VOICE_LANGUAGES.has(language) && (focused || voiceHere);
  const startVoice = () => {
    const el = textRef.current;
    if (!el) return;
    phon.finish();
    void useVoice.getState().start({ cueId: cue.id, language, start: el.selectionStart, end: el.selectionEnd, text: el.value });
  };
  const toCheck = wordsToCheck(cue);
  const selectWord = (w: string) => {
    const el = textRef.current;
    if (!el) return;
    const at = el.value.indexOf(w);
    if (at < 0) return;
    el.focus();
    el.setSelectionRange(at, at + w.length);
  };
  const duration = cue.endMs - cue.startMs;
  const invalid = duration <= 0;
  const empty = !cue.text.trim();

  // Grow the text box with its content, so every line is visible.
  useLayoutEffect(() => {
    if (textRef.current) fitHeight(textRef.current);
  }, [cue.text]);

  // Lines re-wrap when the panel is resized or a font finishes loading: fit again then.
  useEffect(() => {
    const el = textRef.current;
    if (!el) return;
    let width = el.clientWidth;
    const ro = new ResizeObserver(() => {
      if (el.clientWidth !== width) {
        width = el.clientWidth;
        fitHeight(el);
      }
    });
    ro.observe(el);
    const onFonts = () => fitHeight(el);
    document.fonts?.addEventListener("loadingdone", onFonts);
    return () => {
      ro.disconnect();
      document.fonts?.removeEventListener("loadingdone", onFonts);
    };
  }, []);

  const seekHere = () => {
    const { ms, seek } = usePlayhead.getState();
    if (ms < cue.startMs || ms >= cue.endMs) seek(cue.startMs);
  };

  const cls = ["cue", active && "is-active", selected && "is-selected", overlap && "is-overlap", (empty || invalid) && "is-problem", (toCheck.length > 0 || cue.timingCheck) && "has-check"]
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
        <div className={`cue__textwrap ${showMic ? "has-mic" : ""}`}>
          <textarea
            ref={textRef}
            className="cue__text caption-text"
            value={cue.text}
            dir={rtl ? "rtl" : "auto"}
            lang={language}
            rows={1}
            spellCheck={false}
            placeholder={phonetic ? "Type the caption… English letters become Bangla (ami → আমি)" : "Type the caption…"}
            aria-label={`Caption ${index + 1} text`}
            onChange={(e) => setText(cue.id, e.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => {
              setFocused(false);
              phon.finish();
            }}
            onMouseDown={phon.finish}
            onKeyDown={(e) => {
              if (phon.onKeyDown(e)) return;
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                e.preventDefault();
                split(cue.id, { caret: e.currentTarget.selectionStart });
              }
            }}
          />
          {showMic && <MicButton cueId={cue.id} onStart={startVoice} />}
          {phon.comp && <PhoneticStrip comp={phon.comp} onChoose={phon.choose} />}
        </div>
        {voiceHere && <VoiceBar />}
        {cue.timingCheck && (
          <div className="cue__check" role="group" aria-label="Timing to check">
            <span className="cue__check-label">
              <Clock size={13} /> Check timing
            </span>
            <span className="cue__check-note">Auto-sync couldn't hear this caption clearly.</span>
            <button className="cue__ok" onClick={() => markTimingChecked(cue.id)}>
              <Check size={13} weight="bold" /> Looks right
            </button>
          </div>
        )}
        {toCheck.length > 0 && (
          <div className="cue__check" role="group" aria-label="Words to check">
            <span className="cue__check-label">
              <Eye size={13} /> Check
            </span>
            {toCheck.map((w) => (
              <button key={w} className="cue__word caption-text" lang={language} onClick={() => selectWord(w)} title="Select this word">
                {w}
              </button>
            ))}
            <button className="cue__ok" onClick={() => markChecked(cue.id)}>
              <Check size={13} weight="bold" /> Looks right
            </button>
          </div>
        )}
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
