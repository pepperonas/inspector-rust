import { describe, it, expect } from "vitest";
import { initHistory, historyReducer, canUndo, canRedo } from "./editor-history";

describe("editor-history", () => {
  it("initialises with the present and empty stacks", () => {
    const h = initHistory("a");
    expect(h).toEqual({ past: [], present: "a", future: [] });
    expect(canUndo(h)).toBe(false);
    expect(canRedo(h)).toBe(false);
  });

  it("commit pushes the old present to the past and clears the future", () => {
    let h = initHistory("a");
    h = historyReducer(h, { type: "commit", next: "b" });
    expect(h).toEqual({ past: ["a"], present: "b", future: [] });
    expect(canUndo(h)).toBe(true);
  });

  it("undo then redo round-trips", () => {
    let h = initHistory("a");
    h = historyReducer(h, { type: "commit", next: "b" });
    h = historyReducer(h, { type: "undo" });
    expect(h.present).toBe("a");
    expect(canRedo(h)).toBe(true);
    h = historyReducer(h, { type: "redo" });
    expect(h.present).toBe("b");
  });

  it("undo on an empty past is a no-op", () => {
    const h = initHistory("a");
    expect(historyReducer(h, { type: "undo" })).toEqual(h);
  });

  it("redo on an empty future is a no-op", () => {
    const h = initHistory("a");
    expect(historyReducer(h, { type: "redo" })).toEqual(h);
  });

  it("a fresh commit clears a pending redo", () => {
    let h = initHistory("a");
    h = historyReducer(h, { type: "commit", next: "b" });
    h = historyReducer(h, { type: "undo" }); // present "a", future ["b"]
    h = historyReducer(h, { type: "commit", next: "c" });
    expect(h.present).toBe("c");
    expect(canRedo(h)).toBe(false);
  });

  it("caps the past at the limit, dropping the oldest", () => {
    let h = initHistory(0);
    for (let i = 1; i <= 5; i++) h = historyReducer(h, { type: "commit", next: i }, 3);
    // present 5, past capped to the 3 most recent predecessors [2,3,4]
    expect(h.present).toBe(5);
    expect(h.past).toEqual([2, 3, 4]);
  });

  it("reset drops both stacks", () => {
    let h = initHistory("a");
    h = historyReducer(h, { type: "commit", next: "b" });
    h = historyReducer(h, { type: "reset", present: "z" });
    expect(h).toEqual({ past: [], present: "z", future: [] });
  });
});
