import { describe, expect, it } from "vitest";
import {
  calibrationView,
  CALIBRATION_STEPS,
  describeChange,
  traceKindLabel,
  traceSize,
  traceWhen,
} from "./gesture-calibrate";

describe("calibration view", () => {
  it("is idle without a status", () => {
    expect(calibrationView(undefined)).toEqual({ kind: "idle" });
    expect(calibrationView({ state: "idle" })).toEqual({ kind: "idle" });
  });

  it("counts the countdown down in whole seconds, rounding up", () => {
    expect(calibrationView({ state: "running", phase: { phase: "countdown", remaining_ms: 2_100 } })).toEqual({
      kind: "countdown",
      seconds: 3,
    });
  });

  it("names the step and fills the bar from the backend's step length", () => {
    const v = calibrationView({
      state: "running",
      phase: { phase: "step", step: 3, steps: 3, remaining_ms: 2_000, step_ms: 8_000 },
    });
    expect(v).toMatchObject({ kind: "step", index: 3, of: 3, title: "Finger", progress: 0.75, seconds: 2 });
  });

  it("never draws the bar outside 0..1", () => {
    const at = (remaining_ms: number) =>
      calibrationView({ state: "running", phase: { phase: "step", step: 1, steps: 3, remaining_ms, step_ms: 6_000 } });
    expect(at(9_000)).toMatchObject({ progress: 0 });
    expect(at(-5)).toMatchObject({ progress: 1 });
  });

  it("shows a calculating state between the last step and the result", () => {
    expect(calibrationView({ state: "running", phase: { phase: "finished" } })).toEqual({ kind: "computing" });
  });

  it("passes a failure through with its message", () => {
    expect(calibrationView({ state: "failed", error: "zu wenig" })).toEqual({ kind: "failed", error: "zu wenig" });
  });

  it("has a real instruction for every step", () => {
    // The count is pinned against `calibrate::STEPS` from the Rust side.
    for (const s of CALIBRATION_STEPS) expect(s.instruction.length).toBeGreaterThan(20);
  });
});

describe("proposal rows", () => {
  it("labels and formats a change the German way", () => {
    expect(describeChange({ key: "palm_size", current: 2, proposed: 1.9 })).toEqual({
      key: "palm_size",
      label: "Handballen ab Größe",
      from: "2",
      to: "1,9",
      up: false,
    });
  });

  it("shows the thumb zone in percent of the pad height", () => {
    const r = describeChange({ key: "thumb_zone", current: 0.6, proposed: 0.75 });
    expect([r.from, r.to, r.up]).toEqual(["60 %", "75 %", true]);
  });
});

describe("recording names", () => {
  it("reads the time out of either kind of file name", () => {
    expect(traceWhen("trace-20261004-141516.json")).toBe("04.10. 14:15:16");
    expect(traceWhen("trace-unintended-20261004-141516.json")).toBe("04.10. 14:15:16");
    expect(traceWhen("trace-odd.json")).toBe("trace-odd.json");
  });

  it("labels the kind", () => {
    expect(traceKindLabel({ name: "x", bytes: 1, unintended: true })).toBe("Ungewollt");
    expect(traceKindLabel({ name: "x", bytes: 1, unintended: false })).toBe("Aufnahme");
  });

  it("formats sizes and never shows 0 KB", () => {
    expect(traceSize(10)).toBe("1 KB");
    expect(traceSize(40_000)).toBe("39 KB");
    expect(traceSize(1_500_000)).toBe("1,4 MB");
  });
});
