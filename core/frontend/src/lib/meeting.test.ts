import { describe, expect, it } from "vitest";
import { downloadPercent, errorText, formatElapsed, levelOf, modelSizeLabel, sourceLabel } from "./meeting";

describe("meeting helpers", () => {
  it("formats elapsed time", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(65_000)).toBe("1:05");
    expect(formatElapsed(3_723_000)).toBe("1:02:03");
    expect(formatElapsed(-5)).toBe("0:00");
  });

  it("labels the sources", () => {
    expect(sourceLabel("mic")).toBe("MIC");
    expect(sourceLabel("sys")).toBe("SYS");
  });

  it("shows model sizes in decimal units", () => {
    expect(modelSizeLabel(574_041_195)).toBe("574 MB");
    expect(modelSizeLabel(1_624_555_275)).toMatch(/^1[.,]6 GB$/);
  });

  it("download percent is clamped and unknown without a size", () => {
    expect(downloadPercent(0, 0)).toBeNull();
    expect(downloadPercent(50, 200)).toBe(25);
    expect(downloadPercent(300, 200)).toBe(100);
  });

  it("maps known sentinels and keeps unknown text", () => {
    expect(errorText("transcribe.busy")).not.toBe("transcribe.busy");
    expect(errorText("disk full")).toBe("disk full");
    expect(errorText(null)).toBeNull();
  });

  it("level is 0 for silence and grows with loudness", () => {
    expect(levelOf(0)).toBe(0);
    expect(levelOf(1)).toBe(1);
    expect(levelOf(0.03)).toBeGreaterThan(levelOf(0.003));
  });
});
