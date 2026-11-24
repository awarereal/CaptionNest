// Editable timecode. Type a time, or nudge with arrow keys:
// ↑/↓ = 0.1 s, Shift = 1 s, Alt = 0.01 s. Invalid input reverts.
import { useEffect, useState } from "react";
import { formatTimecode, parseTimecode } from "../lib/time";

export function TimeField({
  value,
  onCommit,
  label,
  invalid,
}: {
  value: number;
  onCommit: (ms: number) => void;
  label: string;
  invalid?: boolean;
}) {
  const [text, setText] = useState(formatTimecode(value));
  const [bad, setBad] = useState(false);
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) setText(formatTimecode(value));
  }, [value, focused]);

  const commit = () => {
    const ms = parseTimecode(text);
    if (ms === null) {
      setBad(true);
      setText(formatTimecode(value));
      setTimeout(() => setBad(false), 500);
      return;
    }
    if (ms !== value) onCommit(ms);
    setText(formatTimecode(ms));
  };

  return (
    <input
      className={`timefield mono ${bad ? "is-bad" : ""} ${invalid ? "is-invalid" : ""}`}
      value={text}
      aria-label={label}
      aria-invalid={invalid || bad || undefined}
      spellCheck={false}
      inputMode="decimal"
      onChange={(e) => setText(e.target.value)}
      onFocus={(e) => {
        setFocused(true);
        e.target.select();
      }}
      onBlur={() => {
        setFocused(false);
        commit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
          (e.target as HTMLInputElement).blur();
        } else if (e.key === "Escape") {
          setText(formatTimecode(value));
          (e.target as HTMLInputElement).blur();
        } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
          e.preventDefault();
          const step = e.shiftKey ? 1000 : e.altKey ? 10 : 100;
          const next = Math.max(0, value + (e.key === "ArrowUp" ? step : -step));
          onCommit(next);
          setText(formatTimecode(next));
        }
      }}
    />
  );
}
