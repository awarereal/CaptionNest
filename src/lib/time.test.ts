import { describe, expect, it } from "vitest";
import { formatClock, formatDuration, formatTimecode, parseOffsetSeconds, parseTimecode } from "./time";

describe("time formatting", () => {
  it("formats timecodes", () => {
    expect(formatTimecode(0)).toBe("00:00:00.000");
    expect(formatTimecode(61_234)).toBe("00:01:01.234");
    expect(formatTimecode(3_600_000 + 5)).toBe("01:00:00.005");
    expect(formatTimecode(-5)).toBe("00:00:00.000");
  });

  it("formats clocks and durations", () => {
    expect(formatClock(62_000)).toBe("1:02");
    expect(formatClock(3_723_000)).toBe("1:02:03");
    expect(formatDuration(38_000)).toBe("38 s");
    expect(formatDuration(252_000)).toBe("4 min 12 s");
    expect(formatDuration(3_780_000)).toBe("1 h 3 min");
  });

  it("parses what people type", () => {
    expect(parseTimecode("00:01:01.234")).toBe(61_234);
    expect(parseTimecode("00:01:01,234")).toBe(61_234);
    expect(parseTimecode("1:01.5")).toBe(61_500);
    expect(parseTimecode("12.25")).toBe(12_250);
    expect(parseTimecode("1:02:03")).toBe(3_723_000);
  });

  it("rejects invalid times", () => {
    for (const bad of ["", "abc", "1:60", "00:00:61", "1.2.3", "-1"]) expect(parseTimecode(bad)).toBeNull();
  });

  it("round-trips", () => {
    for (const ms of [0, 999, 59_999, 3_599_999, 7_200_123]) expect(parseTimecode(formatTimecode(ms))).toBe(ms);
  });

  it("parses offsets in seconds", () => {
    expect(parseOffsetSeconds("+1.5")).toBe(1500);
    expect(parseOffsetSeconds("-0.25")).toBe(-250);
    expect(parseOffsetSeconds("2")).toBe(2000);
    expect(parseOffsetSeconds("0,5")).toBe(500);
    expect(parseOffsetSeconds("x")).toBeNull();
    expect(parseOffsetSeconds("-")).toBeNull();
  });
});
