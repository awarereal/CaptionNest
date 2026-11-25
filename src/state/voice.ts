// Voice typing: the microphone's sound streams to Deepgram (through Rust) while you speak,
// and the words appear in the caption at the cursor as you say them: a running guess that
// Deepgram corrects, made final at each short pause.
//
// The whole recording is kept too. If the live connection fails, it's sent after "Stop"
// and written down at once (Deepgram's pre-recorded mode), so nothing said is lost. Cancel
// takes the live words out again; editing the caption meanwhile ends dictation and keeps
// the words so far. The Deepgram key stays in the Rust backend; this side only knows
// whether one is saved.
import { create } from "zustand";
import { api, toAppError } from "../lib/api";
import { useStore } from "./store";
import { insertWords } from "../lib/insertWords";
import type { VoiceLiveEvent, VoiceStatus } from "../lib/types";

export type VoicePhase = "idle" | "listening" | "processing" | "stopped" | "error";

/** Caption languages voice typing writes (Deepgram Nova-3 codes we use). */
export const VOICE_LANGUAGES = new Set(["bn", "en"]);

/** Where the words go: the caption and the selection when the microphone was clicked. */
export interface VoiceTarget {
  cueId: string;
  language: string;
  start: number;
  end: number;
  /** The caption text at that moment, to tell whether it changed meanwhile. */
  text: string;
}

export interface VoiceError {
  message: string;
  /** What the user can do: add a key, record again, send the same recording again, or nothing. */
  fix: "settings" | "retry" | "resend" | null;
}

/** Recordings stop by themselves after this long. */
const MAX_MS = 5 * 60_000;
/** Shorter than this is a slip of the button, not speech. */
const MIN_MS = 500;

interface VoiceState {
  status: VoiceStatus | null;
  phase: VoicePhase;
  target: VoiceTarget | null;
  /** Microphone loudness 0–1 for the level meter. */
  level: number;
  startedAt: number;
  error: VoiceError | null;
  /** What happened, shown in the Stopped state. */
  note: string | null;
  /** Words are appearing while you speak (the live connection is open). */
  live: boolean;
  refreshStatus: () => Promise<void>;
  start: (target: VoiceTarget) => Promise<void>;
  stop: () => void;
  cancel: () => void;
  /** Sends the last recording again (after a connection or service problem). */
  resend: () => void;
  dismiss: () => void;
}

/** The last recording that couldn't be written down, kept so it can be sent again. */
let unsent: { bytes: Uint8Array; type: string; target: VoiceTarget } | null = null;

// The recording in progress (not React state: none of it is drawn directly).
let stream: MediaStream | null = null;
let recorder: MediaRecorder | null = null;
let chunks: Blob[] = [];
let audio: AudioContext | null = null;
let meterFrame = 0;
let limitTimer = 0;
let noteTimer = 0;
/** Bumped by every new recording and every cancel, so stale results are ignored. */
let run = 0;

// Live dictation.
let liveId: number | null = null;
/** False once the live connection failed: the recording is written down after Stop instead. */
let liveOk = false;
/** Sound recorded before the connection was open, sent once it is. */
let pendingPcm: Uint8Array[] = [];
/** The next sound piece's number (the backend puts pieces back in order). */
let liveSeq = 0;
/** Final words so far, and Deepgram's running guess at what follows. */
let committed = "";
let guess = "";
/** The caption text as voice typing last wrote it, to notice the user editing meanwhile. */
let written: string | null = null;
let edited = false;
let proc: ScriptProcessorNode | null = null;
let unlistenLive: (() => void) | null = null;
let liveEnded: (() => void) | null = null;

const joinWords = (a: string, b: string) => [a.trim(), b.trim()].filter(Boolean).join(" ");

function releaseLive() {
  unlistenLive?.();
  unlistenLive = null;
  liveId = null;
  pendingPcm = [];
  liveSeq = 0;
}

/** Microphone samples (-1..1) as 16-bit little-endian PCM. */
function toPcm16(samples: Float32Array): Uint8Array {
  const out = new Int16Array(samples.length);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    out[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }
  return new Uint8Array(out.buffer);
}

function release() {
  proc?.disconnect();
  if (proc) proc.onaudioprocess = null;
  proc = null;
  cancelAnimationFrame(meterFrame);
  clearTimeout(limitTimer);
  stream?.getTracks().forEach((t) => t.stop());
  void audio?.close().catch(() => undefined);
  stream = null;
  recorder = null;
  audio = null;
}

const isMac = navigator.userAgent.includes("Mac");

function micError(e: unknown): VoiceError {
  const name = e instanceof DOMException ? e.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") {
    return {
      message: isMac
        ? "CaptionNest isn't allowed to use the microphone. Turn it on in System Settings → Privacy & Security → Microphone, then try again."
        : "CaptionNest isn't allowed to use the microphone. Turn it on in Windows Settings → Privacy & security → Microphone, then try again.",
      fix: "retry",
    };
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return { message: "No microphone was found. Connect one and try again.", fix: "retry" };
  }
  if (name === "NotReadableError" || name === "AbortError") {
    return { message: "The microphone is busy in another app. Close that app's recording and try again.", fix: "retry" };
  }
  return { message: "The microphone couldn't be started. Try again.", fix: "retry" };
}

/** The best container this web view can record speech in. */
function recordingType(): string {
  for (const t of ["audio/webm;codecs=opus", "audio/mp4", "audio/webm", "audio/ogg;codecs=opus"]) {
    if (typeof MediaRecorder !== "undefined" && MediaRecorder.isTypeSupported(t)) return t;
  }
  return "";
}

export const useVoice = create<VoiceState>((set, get) => {
  const showNote = (note: string) => {
    set({ phase: "stopped", note, error: null, level: 0 });
    clearTimeout(noteTimer);
    const mine = run;
    noteTimer = window.setTimeout(() => {
      if (run === mine && get().phase === "stopped") set({ phase: "idle", target: null, note: null });
    }, 3200);
  };

  const fail = (error: VoiceError) => {
    release();
    set({ phase: "error", error, level: 0 });
  };

  /** Puts the words into the caption and moves the cursor after them. */
  const deliver = (target: VoiceTarget, words: string) => {
    const st = useStore.getState();
    const track = st.history.present.find((t) => t.id === st.activeTrackId);
    const cue = track?.cues.find((c) => c.id === target.cueId);
    if (!cue) {
      fail({ message: `That caption isn't open anymore, so these words weren't added: “${words}”`, fix: null });
      return;
    }
    const { text, caret } = insertWords(cue.text, target, words);
    st.setCueText(cue.id, text);
    requestAnimationFrame(() => {
      const el = document.querySelector<HTMLTextAreaElement>(`[data-cue-id="${CSS.escape(cue.id)}"] textarea`);
      if (!el) return;
      el.focus();
      el.setSelectionRange(caret, caret);
    });
    const count = words.split(/\s+/).filter(Boolean).length;
    showNote(`Added ${count} ${count === 1 ? "word" : "words"}. Edit them like any other text.`);
  };

  const findCue = (target: VoiceTarget) => {
    const st = useStore.getState();
    return st.history.present.find((t) => t.id === st.activeTrackId)?.cues.find((c) => c.id === target.cueId);
  };

  /** Writes the words heard so far into the caption at the cursor, replacing the last guess. */
  const writeLive = (target: VoiceTarget, words: string): boolean => {
    const cue = findCue(target);
    if (!cue) return false;
    if (cue.text !== (written ?? target.text)) {
      edited = true;
      return false;
    }
    const { text, caret } = words ? insertWords(target.text, target, words) : { text: target.text, caret: target.end };
    if (text !== cue.text) useStore.getState().setCueText(cue.id, text);
    written = text;
    requestAnimationFrame(() => {
      const el = document.querySelector<HTMLTextAreaElement>(`[data-cue-id="${CSS.escape(cue.id)}"] textarea`);
      if (!el) return;
      if (document.activeElement !== el) el.focus();
      el.setSelectionRange(caret, caret);
    });
    return true;
  };

  /** Takes the live words out of the caption again (unless the user changed it since). */
  const unwriteLive = (target: VoiceTarget) => {
    const cue = findCue(target);
    if (cue && written !== null && cue.text === written && cue.text !== target.text) useStore.getState().setCueText(cue.id, target.text);
    written = null;
  };

  const onLive = (mine: number, target: VoiceTarget, ev: VoiceLiveEvent) => {
    if (run !== mine || ev.id !== liveId) return;
    if (ev.kind === "end") {
      liveEnded?.();
      return;
    }
    if (ev.kind === "error") {
      liveOk = false;
      set({ live: false });
      return;
    }
    if (ev.isFinal) {
      committed = joinWords(committed, ev.text);
      guess = "";
    } else {
      guess = ev.text;
    }
    if (edited || get().phase !== "listening") return;
    // Editing the caption while speaking ends dictation; the words so far stay.
    if (!writeLive(target, joinWords(committed, guess)) && edited) get().stop();
  };

  const process = async (mine: number, target: VoiceTarget, type: string, ms: number) => {
    const blob = new Blob(chunks, { type: type || chunks[0]?.type || "" });
    chunks = [];
    release();
    if (run !== mine) return;
    if (liveOk && liveId !== null) {
      const id = liveId;
      // Deepgram writes down the last words, then ends the stream.
      await new Promise<void>((done) => {
        liveEnded = done;
        void api.finishVoiceLive(id).catch(() => done());
        window.setTimeout(done, 8000);
      });
      liveEnded = null;
      if (run !== mine) return;
      releaseLive();
      if (edited) {
        showNote("Voice typing stopped because the caption was edited. The words so far were kept.");
        return;
      }
      if (liveOk) {
        const words = joinWords(committed, guess);
        writeLive(target, words);
        const count = words.split(/\s+/).filter(Boolean).length;
        if (count === 0) showNote(ms < MIN_MS ? "Stopped. Nothing was added: the recording was too short." : "Stopped. No words were heard.");
        else showNote(`Added ${count} ${count === 1 ? "word" : "words"}. Edit them like any other text.`);
        return;
      }
    }
    // The live connection didn't work: write down the whole recording instead.
    releaseLive();
    unwriteLive(target);
    if (ms < MIN_MS || blob.size === 0) {
      showNote("Stopped. Nothing was added: the recording was too short.");
      return;
    }
    await send(mine, new Uint8Array(await blob.arrayBuffer()), blob.type, target);
  };

  const send = async (mine: number, bytes: Uint8Array, type: string, target: VoiceTarget) => {
    set({ phase: "processing", level: 0, error: null });
    unsent = null;
    try {
      const heard = await api.transcribeVoice(bytes, type, target.language);
      if (run !== mine) return;
      deliver(target, heard.text.trim());
    } catch (e) {
      if (run !== mine) return;
      const err = toAppError(e);
      const keyProblem = err.code === "voice_key" || err.code === "voice_credits";
      // Nothing was heard: speak again. Connection or service trouble: the same recording can be sent again.
      const fix = keyProblem ? "settings" : err.code === "no_speech" ? "retry" : "resend";
      // Kept for "Send again", also after a key is added in Settings.
      if (fix !== "retry") unsent = { bytes, type, target };
      set({ phase: "error", error: { message: err.message, fix }, level: 0 });
    }
  };

  return {
    status: null,
    phase: "idle",
    target: null,
    level: 0,
    startedAt: 0,
    error: null,
    note: null,
    live: false,

    async refreshStatus() {
      try {
        set({ status: await api.voiceStatus() });
      } catch {
        // Voice typing stays available; the key check happens when it's used.
      }
    },

    async start(target) {
      const { phase } = get();
      if (phase === "listening" || phase === "processing") return;
      unsent = null;
      const mine = ++run;
      clearTimeout(noteTimer);
      set({ target, phase: "listening", error: null, note: null, level: 0, startedAt: Date.now(), live: false });
      committed = "";
      guess = "";
      written = null;
      edited = false;
      liveOk = false;
      releaseLive();

      if (!get().status?.keySaved) {
        await get().refreshStatus();
        if (!get().status?.keySaved) {
          fail({ message: "Voice typing uses Deepgram. Add your Deepgram API key in Settings → Voice typing to start.", fix: "settings" });
          return;
        }
      }
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
        fail({ message: "Recording from the microphone isn't available on this computer.", fix: null });
        return;
      }
      try {
        stream = await navigator.mediaDevices.getUserMedia({
          audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
        });
      } catch (e) {
        if (run === mine) fail(micError(e));
        else release();
        return;
      }
      if (run !== mine) {
        release();
        return;
      }

      const type = recordingType();
      try {
        recorder = new MediaRecorder(stream, type ? { mimeType: type, audioBitsPerSecond: 128_000 } : undefined);
      } catch (e) {
        fail(micError(e));
        return;
      }
      chunks = [];
      const startedAt = Date.now();
      recorder.ondataavailable = (ev) => {
        if (ev.data.size > 0) chunks.push(ev.data);
      };
      recorder.onstop = () => void process(mine, target, recorder?.mimeType || type, Date.now() - startedAt);
      recorder.start(1000);
      set({ startedAt });

      // Level meter, so it's obvious the microphone hears you, and the sound for live words.
      try {
        try {
          // 16 kHz is all speech recognition needs, and a quarter of the data to send.
          audio = new AudioContext({ sampleRate: 16_000 });
        } catch {
          audio = new AudioContext();
        }
        const source = audio.createMediaStreamSource(stream);
        const analyser = audio.createAnalyser();
        analyser.fftSize = 1024;
        source.connect(analyser);
        try {
          proc = audio.createScriptProcessor(2048, 1, 1);
          const mute = audio.createGain();
          mute.gain.value = 0;
          source.connect(proc);
          proc.connect(mute);
          mute.connect(audio.destination);
          proc.onaudioprocess = (e) => {
            if (run !== mine || !liveOk) return;
            const pcm = toPcm16(e.inputBuffer.getChannelData(0));
            if (liveId === null) {
              // About 50 s at 16 kHz: more than any connection takes.
              if (pendingPcm.length < 400) pendingPcm.push(pcm);
              return;
            }
            void api.voiceLiveAudio(liveId, liveSeq++, pcm).catch(() => undefined);
          };
          liveOk = true;
        } catch {
          liveOk = false;
        }
        const buf = new Float32Array(analyser.fftSize);
        let last = 0;
        const tick = (now: number) => {
          analyser.getFloatTimeDomainData(buf);
          let sum = 0;
          for (const v of buf) sum += v * v;
          const rms = Math.sqrt(sum / buf.length);
          if (now - last > 50) {
            last = now;
            set({ level: Math.min(1, rms * 6) });
          }
          meterFrame = requestAnimationFrame(tick);
        };
        meterFrame = requestAnimationFrame(tick);
      } catch {
        // The meter is a nicety; recording works without it.
      }
      limitTimer = window.setTimeout(() => get().stop(), MAX_MS);

      // Live words: open the stream to Deepgram. Sound recorded meanwhile is sent once it's open.
      if (liveOk && audio) {
        const rate = audio.sampleRate;
        try {
          const unlisten = await api.onVoiceLive((ev) => onLive(mine, target, ev));
          if (run !== mine) {
            unlisten();
            return;
          }
          unlistenLive = unlisten;
          const id = await api.startVoiceLive(target.language, rate);
          if (run !== mine) {
            void api.cancelVoiceLive(id).catch(() => undefined);
            return;
          }
          liveId = id;
          for (const p of pendingPcm) void api.voiceLiveAudio(id, liveSeq++, p).catch(() => undefined);
          pendingPcm = [];
          if (get().phase === "listening") set({ live: true });
        } catch (e) {
          liveOk = false;
          pendingPcm = [];
          if (run !== mine) return;
          const err = toAppError(e);
          // A key problem stops here; anything else falls back to writing it down after Stop.
          if (err.code === "voice_key" || err.code === "voice_credits") {
            run++;
            if (recorder) {
              recorder.onstop = null;
              if (recorder.state !== "inactive") recorder.stop();
            }
            chunks = [];
            releaseLive();
            fail({ message: err.message, fix: "settings" });
          }
        }
      }
    },

    stop() {
      if (get().phase !== "listening") return;
      if (recorder && recorder.state !== "inactive") {
        set({ phase: "processing", level: 0 });
        recorder.stop();
      } else {
        get().cancel();
      }
    },

    cancel() {
      const { phase, target } = get();
      run++;
      if (liveId !== null) void api.cancelVoiceLive(liveId).catch(() => undefined);
      liveEnded?.();
      releaseLive();
      // Cancel adds nothing: live words already written come out again.
      if (target && (phase === "listening" || phase === "processing")) unwriteLive(target);
      if (recorder) {
        recorder.onstop = null;
        if (recorder.state !== "inactive") recorder.stop();
      }
      chunks = [];
      release();
      if (phase === "listening" || phase === "processing") showNote("Stopped. Nothing was added.");
      else set({ phase: "idle", target: null, error: null, note: null, level: 0 });
    },

    resend() {
      if (!unsent) {
        // Nothing was recorded yet (e.g. the key was missing): record now.
        const target = get().target;
        if (target) void get().start(target);
        return;
      }
      const { bytes, type, target } = unsent;
      const mine = ++run;
      set({ target });
      void send(mine, bytes, type, target);
    },

    dismiss() {
      clearTimeout(noteTimer);
      unsent = null;
      set({ phase: "idle", target: null, error: null, note: null, level: 0 });
    },
  };
});

// A recording belongs to one caption on screen. Leaving it (another project, another
// track, the Style panel, another screen) cancels the recording, so the microphone is
// never left on without a visible Stop button.
useStore.subscribe((now, before) => {
  const leaving =
    now.project?.id !== before.project?.id ||
    now.activeTrackId !== before.activeTrackId ||
    now.view !== before.view ||
    (now.styleOpen && !before.styleOpen);
  const { phase } = useVoice.getState();
  if (leaving && (phase === "listening" || phase === "processing")) useVoice.getState().cancel();
});
