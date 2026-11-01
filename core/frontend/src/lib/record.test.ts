import { describe, expect, it } from "vitest";
import { canStart, displayElapsed, formatElapsed, phaseLabelKey } from "./record";

describe("record lib", () => {
  it("formats elapsed time", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(61_000)).toBe("1:01");
    expect(formatElapsed(3_661_000)).toBe("1:01:01");
    expect(formatElapsed(-5)).toBe("0:00");
  });
  it("canStart needs a source, idle and support", () => {
    expect(canStart({ phase: "idle", source: "Display" })).toBe(true);
    expect(canStart({ phase: "idle", source: null })).toBe(false);
    expect(canStart({ phase: "recording", source: "x" })).toBe(false);
    expect(canStart({ phase: "idle", source: "x", supported: false })).toBe(false);
  });
  it("has a label for every phase", () => {
    expect(phaseLabelKey("paused")).toBe("rec2.phase.paused");
    expect(phaseLabelKey("finishing")).toBe("rec2.phase.finishing");
  });
  it("displayElapsed only advances while recording", () => {
    expect(displayElapsed(1000, 0, 500, "recording")).toBe(1500);
    expect(displayElapsed(1000, 0, 500, "paused")).toBe(1000);
  });
});
