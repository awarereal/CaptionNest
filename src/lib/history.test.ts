import { describe, expect, it } from "vitest";
import { canRedo, canUndo, createHistory, LIMIT, record, redo, undo } from "./history";

describe("undo history", () => {
  it("undoes and redoes", () => {
    let h = createHistory(1);
    h = record(h, 2);
    h = record(h, 3);
    expect(h.present).toBe(3);
    h = undo(h);
    expect(h.present).toBe(2);
    h = undo(h);
    expect(h.present).toBe(1);
    expect(canUndo(h)).toBe(false);
    h = redo(h);
    expect(h.present).toBe(2);
    expect(canRedo(h)).toBe(true);
  });

  it("a new edit clears redo", () => {
    let h = record(record(createHistory("a"), "b"), "c");
    h = undo(h);
    h = record(h, "d");
    expect(canRedo(h)).toBe(false);
    expect(undo(h).present).toBe("b");
  });

  it("typing in one caption becomes one undo step", () => {
    let h = createHistory("");
    h = record(h, "H", "text:1", 1000);
    h = record(h, "He", "text:1", 1100);
    h = record(h, "Hey", "text:1", 1200);
    expect(undo(h).present).toBe("");
  });

  it("pauses and other captions start new steps", () => {
    let h = createHistory("");
    h = record(h, "a", "text:1", 1000);
    h = record(h, "ab", "text:1", 5000);
    expect(undo(h).present).toBe("a");
    h = record(h, "x", "text:2", 5100);
    expect(undo(h).present).toBe("ab");
  });

  it("keeps a bounded number of steps", () => {
    let h = createHistory(0);
    for (let i = 1; i <= LIMIT + 50; i++) h = record(h, i);
    expect(h.past.length).toBe(LIMIT);
  });
});
