import { describe, expect, it } from "vitest";
import { frameAt, looksLikeLink } from "./links";
import type { Storyboard } from "./types";

describe("looksLikeLink", () => {
  it("accepts video page addresses", () => {
    for (const t of [
      "https://www.youtube.com/watch?v=aqz-KE-bpKQ&t=90",
      "  https://youtu.be/aqz-KE-bpKQ ",
      "youtu.be/aqz-KE-bpKQ",
      "www.youtube.com/shorts/abc",
      "https://vimeo.com/1084537",
      "http://example.org/video",
    ])
      expect(looksLikeLink(t), t).toBe(true);
  });

  it("ignores ordinary text", () => {
    for (const t of ["", "hello world", "আজ আমরা নদীর ধারে", "file:///Users/me/video.mp4", "C:\\Videos\\a.mp4", "youtube"]) expect(looksLikeLink(t), t).toBe(false);
  });
});

describe("frameAt", () => {
  // YouTube's 160 px storyboard: 5 × 5 frames per sheet, one frame every ~4.96 s.
  const sb: Storyboard = { sheets: ["s0", "s1", "s2"], columns: 5, rows: 5, width: 160, height: 90, frameMs: 1000 / 0.2015748 };

  it("finds the sheet, column and row", () => {
    expect(frameAt(sb, 0)).toEqual({ url: "s0", col: 0, row: 0 });
    expect(frameAt(sb, 6 * sb.frameMs + 1)).toEqual({ url: "s0", col: 1, row: 1 });
    expect(frameAt(sb, 25 * sb.frameMs + 1)).toEqual({ url: "s1", col: 0, row: 0 });
  });

  it("stays on the last frame past the end", () => {
    expect(frameAt(sb, 10_000_000)).toEqual({ url: "s2", col: 4, row: 4 });
    expect(frameAt(sb, -5)).toEqual({ url: "s0", col: 0, row: 0 });
  });
});
