// Playback position lives in its own tiny store: it changes every animation frame, and
// only the few components that show time subscribe to it.
import { create } from "zustand";

interface Playhead {
  ms: number;
  playing: boolean;
  /** Set by the player; lets anything seek without holding a ref. */
  seek: (ms: number) => void;
  togglePlay: () => void;
  set: (p: Partial<Pick<Playhead, "ms" | "playing">>) => void;
  bind: (handlers: { seek: (ms: number) => void; togglePlay: () => void }) => void;
}

export const usePlayhead = create<Playhead>((set) => ({
  ms: 0,
  playing: false,
  seek: () => {},
  togglePlay: () => {},
  set: (p) => set(p),
  bind: (h) => set({ seek: h.seek, togglePlay: h.togglePlay }),
}));
