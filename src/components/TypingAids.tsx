// Helpers shown with a caption's text box: Bangla spelling choices while typing
// phonetically, the voice-typing microphone, and the voice-typing status bar.
import { useEffect, useState } from "react";
import { CheckCircle, Microphone, Stop, WarningCircle, X } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { useVoice } from "../state/voice";
import { Button, IconButton, Tip } from "./ui";
import { useBanglaTyping, type Composition } from "./usePhonetic";

/** The word being typed in Latin letters and the Bangla spellings to choose from. */
export function PhoneticStrip({ comp, onChoose }: { comp: Composition; onChoose: (i: number) => void }) {
  return (
    <div className="typestrip" role="listbox" aria-label={`Bangla spellings for ${comp.roman}`}>
      <span className="typestrip__roman mono">{comp.roman}</span>
      <span className="typestrip__options">
        {comp.options.map((o, i) => (
          <button
            key={o}
            role="option"
            aria-selected={i === comp.index}
            className={`typestrip__opt caption-text ${i === comp.index ? "is-on" : ""}`}
            lang="bn"
            // Keep the caret in the text box.
            onMouseDown={(e) => {
              e.preventDefault();
              onChoose(i);
            }}
          >
            {o}
          </button>
        ))}
      </span>
      <span className="typestrip__hint">
        {comp.options.length > 1 && <>↑↓ other spellings · </>}Esc keeps English
      </span>
    </div>
  );
}

/** Switches Bangla phonetic typing on or off (shown for Bangla tracks). */
export function BanglaTypingToggle() {
  const on = useBanglaTyping((s) => s.on);
  const toggle = useBanglaTyping((s) => s.toggle);
  return (
    <Tip label={on ? "Bangla typing is on: type ami to get আমি" : "Bangla typing is off: letters stay English"} shortcut="Mod+B">
      <button className={`typetoggle ${on ? "is-on" : ""}`} aria-pressed={on} aria-label="Bangla typing" onClick={toggle}>
        <span className="typetoggle__glyph" lang="bn" aria-hidden>
          অ
        </span>
        <span className="typetoggle__label">Bangla typing</span>
      </button>
    </Tip>
  );
}

/** The microphone beside the caption being edited. */
export function MicButton({ onStart, cueId }: { onStart: () => void; cueId: string }) {
  const here = useVoice((s) => s.target?.cueId === cueId);
  const voicePhase = useVoice((s) => s.phase);
  const busy = voicePhase === "listening" || voicePhase === "processing";
  // Another caption is being dictated: this microphone waits.
  const phase = busy && !here ? "processing" : voicePhase;
  return (
    <IconButton
      label={phase === "listening" ? "Stop and add the words" : "Voice typing: speak and your words are added here"}
      size="sm"
      side="left"
      tone={phase === "listening" ? "active" : "default"}
      className={`micbtn ${phase === "listening" ? "is-listening" : ""}`}
      disabled={phase === "processing"}
      // Keep the text box focused and its selection where it is.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => (busy ? useVoice.getState().stop() : onStart())}
    >
      {phase === "listening" ? <Stop size={14} weight="fill" /> : <Microphone size={15} weight="fill" />}
    </IconButton>
  );
}

function useElapsed(since: number, running: boolean): string {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const t = window.setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [running]);
  const s = Math.max(0, Math.floor((now - since) / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Listening → Processing → Stopped (or a problem), under the caption being dictated. */
export function VoiceBar() {
  const { phase, level, startedAt, error, note, stop, cancel, dismiss } = useVoice();
  const hasKey = useVoice((s) => !!s.status?.keySaved);
  const elapsed = useElapsed(startedAt, phase === "listening");

  // Esc cancels while listening or processing; nothing is added.
  useEffect(() => {
    if (phase !== "listening" && phase !== "processing") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        cancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [phase, cancel]);

  if (phase === "idle") return null;

  if (phase === "listening") {
    const bars = [0.35, 0.7, 1, 0.7, 0.35];
    return (
      <div className="voicebar is-listening" role="status" aria-live="polite">
        <span className="voicebar__dot" aria-hidden />
        <span className="voicebar__state">Listening</span>
        <span className="voicebar__time mono">{elapsed}</span>
        <span className="voicebar__meter" aria-hidden>
          {bars.map((b, i) => (
            <span key={i} style={{ transform: `scaleY(${Math.max(0.12, Math.min(1, level * 1.6 * b))})` }} />
          ))}
        </span>
        <span className="voicebar__hint">Speak in Bangla, then stop.</span>
        <span className="voicebar__actions">
          <Button size="sm" variant="ghost" onMouseDown={(e) => e.preventDefault()} onClick={cancel}>
            Cancel
          </Button>
          <Button size="sm" variant="primary" icon={<Stop size={13} weight="fill" />} onMouseDown={(e) => e.preventDefault()} onClick={stop}>
            Stop
          </Button>
        </span>
      </div>
    );
  }

  if (phase === "processing") {
    return (
      <div className="voicebar is-processing" role="status" aria-live="polite">
        <span className="spinner" aria-hidden />
        <span className="voicebar__state">Processing</span>
        <span className="voicebar__hint">Writing down what you said…</span>
        <span className="voicebar__actions">
          <Button size="sm" variant="ghost" onMouseDown={(e) => e.preventDefault()} onClick={cancel}>
            Cancel
          </Button>
        </span>
      </div>
    );
  }

  if (phase === "stopped") {
    return (
      <div className="voicebar is-stopped" role="status" aria-live="polite">
        <CheckCircle size={16} weight="fill" />
        <span className="voicebar__state">Stopped</span>
        <span className="voicebar__hint">{note}</span>
        <span className="voicebar__actions">
          <IconButton label="Dismiss" size="sm" side="left" onClick={dismiss}>
            <X size={13} />
          </IconButton>
        </span>
      </div>
    );
  }

  return (
    <div className="voicebar is-error" role="alert">
      <WarningCircle size={16} weight="fill" />
      <span className="voicebar__message">{error?.message}</span>
      <span className="voicebar__actions">
        {error?.fix === "settings" && (
          <Button size="sm" variant="secondary" onClick={() => useStore.getState().openPanel("settings", "voice")}>
            Open Settings
          </Button>
        )}
        {(error?.fix === "resend" || (error?.fix === "settings" && hasKey)) && (
          <Button size="sm" variant="secondary" onClick={() => useVoice.getState().resend()}>
            Send again
          </Button>
        )}
        {error?.fix === "retry" && (
          <Button size="sm" variant="secondary" onClick={() => {
            const target = useVoice.getState().target;
            if (target) void useVoice.getState().start(target);
          }}>
            Try again
          </Button>
        )}
        <IconButton label="Dismiss" size="sm" side="left" onClick={dismiss}>
          <X size={13} />
        </IconButton>
      </span>
    </div>
  );
}
