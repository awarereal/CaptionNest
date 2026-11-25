import { describe, expect, it } from "vitest";
import { activeIndex, addCue, deleteCue, mergeWithNext, offsetAll, overlappingIds, retimeCue, sortCues, splitCue, updateCue } from "./cueOps";
import type { Cue } from "./types";

const cue = (id: string, startMs: number, endMs: number, text = id): Cue => ({ id, startMs, endMs, text });

describe("cue editing", () => {
  const base = [cue("a", 0, 1000, "Hello there"), cue("b", 1500, 3000, "General Kenobi"), cue("c", 4000, 5000, "আমি বাংলায় কথা বলি।")];

  it("edits text without touching other cues", () => {
    const out = updateCue(base, "b", { text: "Changed" });
    expect(out[1].text).toBe("Changed");
    expect(out[0]).toBe(base[0]);
  });

  it("retimes and keeps start order", () => {
    const out = retimeCue(base, "c", 200, 800);
    expect(out.map((c) => c.id)).toEqual(["a", "c", "b"]);
  });

  it("clamps negative times to zero", () => {
    expect(retimeCue(base, "a", -500, 300)[0].startMs).toBe(0);
  });

  it("adds a cue that fits before the next one", () => {
    const { cues, id } = addCue(base, 3200);
    const added = cues.find((c) => c.id === id)!;
    expect(added.startMs).toBe(3200);
    expect(added.endMs).toBe(4000);
    expect(added.text).toBe("");
    expect(cues.map((c) => c.startMs)).toEqual([0, 1500, 3200, 4000]);
  });

  it("deletes a cue", () => {
    expect(deleteCue(base, "b").map((c) => c.id)).toEqual(["a", "c"]);
  });

  it("splits at the caret and at the playhead", () => {
    const out = splitCue(base, "b", { caret: 7, atMs: 2000 });
    expect(out).toHaveLength(4);
    expect([out[1].text, out[2].text]).toEqual(["General", "Kenobi"]);
    expect([out[1].endMs, out[2].startMs, out[2].endMs]).toEqual([2000, 2000, 3000]);
  });

  it("splits Bangla text at a word boundary with proportional timing", () => {
    const out = splitCue(base, "c");
    const [x, y] = [out[2], out[3]];
    expect(x.text + " " + y.text).toBe("আমি বাংলায় কথা বলি।");
    expect(x.endMs).toBe(y.startMs);
    expect(x.endMs).toBeGreaterThan(4000);
    expect(y.endMs).toBe(5000);
  });

  it("merges with the next cue", () => {
    const out = mergeWithNext(base, "a");
    expect(out).toHaveLength(2);
    expect(out[0]).toMatchObject({ startMs: 0, endMs: 3000, text: "Hello there General Kenobi" });
    expect(mergeWithNext(base, "c")).toBe(base);
  });

  it("shifts all cues and keeps them at or after zero", () => {
    expect(offsetAll(base, 500).map((c) => c.startMs)).toEqual([500, 2000, 4500]);
    const back = offsetAll(base, -1200);
    expect(back[0].startMs).toBe(0);
    expect(back[0].endMs).toBeGreaterThanOrEqual(200);
    expect(back[1].startMs).toBe(300);
  });

  it("finds overlapping cues", () => {
    const list = [cue("a", 0, 2000), cue("b", 1500, 3000), cue("c", 3000, 4000)];
    expect([...overlappingIds(list)].sort()).toEqual(["a", "b"]);
  });

  it("finds the cue showing at a time", () => {
    const sorted = sortCues(base);
    expect(activeIndex(sorted, 500)).toBe(0);
    expect(activeIndex(sorted, 1200)).toBe(-1);
    expect(activeIndex(sorted, 4999)).toBe(2);
    expect(activeIndex(sorted, 5000)).toBe(-1);
    // Overlap: the earlier caption shows until it ends, as in the exported video.
    const overlapping = sortCues([cue("a", 0, 2000), cue("b", 1500, 3000)]);
    expect(activeIndex(overlapping, 1700)).toBe(0);
    expect(activeIndex(overlapping, 2100)).toBe(1);
  });
});
