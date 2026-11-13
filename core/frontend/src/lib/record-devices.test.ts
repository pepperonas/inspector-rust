import { describe, expect, it } from "vitest";
import {
  deviceMissing, parsePreviewFrame, PEAK_FALL_PER_S, PEAK_HOLD_MS, peakStep, sameDevices, shownDevice, storedName,
} from "./record-devices";

const list = [{ id: "A", name: "Built-in" }, { id: "USB Audio#2", name: "USB Audio" }];

describe("record devices", () => {
  it("a poll that changed nothing is the same list", () => {
    expect(sameDevices(list, [...list.map((d) => ({ ...d }))])).toBe(true);
    expect(sameDevices(list, list.slice(0, 1))).toBe(false);
    expect(sameDevices(list, [list[1], list[0]])).toBe(false);
    expect(sameDevices(list, [list[0], { id: "USB Audio#2", name: "Renamed" }])).toBe(false);
    expect(sameDevices([], [])).toBe(true);
  });

  it("the stored device is shown only while it is connected", () => {
    expect(shownDevice(list, "USB Audio#2")).toBe("USB Audio#2");
    expect(shownDevice(list, "Gone")).toBe("");
    expect(shownDevice(list, "")).toBe("");
    expect(shownDevice(list, undefined)).toBe("");
  });

  it("a missing device is named only once the list is known", () => {
    expect(deviceMissing(list, "Gone", true)).toBe(true);
    expect(deviceMissing(list, "Gone", false)).toBe(false);
    expect(deviceMissing(list, "A", true)).toBe(false);
    expect(deviceMissing(list, "", true)).toBe(false);
    expect(storedName("USB Audio#2")).toBe("USB Audio");
    expect(storedName("Yeti #1 Mic")).toBe("Yeti #1 Mic");
  });

  it("a preview answer is generation + JPEG", () => {
    const buf = new ArrayBuffer(8 + 4);
    new DataView(buf).setBigUint64(0, 42n, true);
    new Uint8Array(buf, 8).set([0xff, 0xd8, 1, 2]);
    const f = parsePreviewFrame(buf);
    expect(f?.gen).toBe(42);
    expect(Array.from(f?.jpeg ?? [])).toEqual([0xff, 0xd8, 1, 2]);
    expect(parsePreviewFrame(new ArrayBuffer(0))).toBeNull();
    expect(parsePreviewFrame(new ArrayBuffer(8))).toBeNull();
    const notJpeg = new ArrayBuffer(12);
    new Uint8Array(notJpeg, 8).set([1, 2, 3, 4]);
    expect(parsePreviewFrame(notJpeg)).toBeNull();
  });

  it("the peak jumps up, holds, then falls no lower than the bar", () => {
    let p = peakStep({ level: 0, heldMs: 0 }, 0.8, 80);
    expect(p).toEqual({ level: 0.8, heldMs: 0 });
    p = peakStep(p, 0.2, PEAK_HOLD_MS);
    expect(p.level).toBe(0.8);
    p = peakStep(p, 0.2, 500);
    expect(p.level).toBeCloseTo(0.8 - PEAK_FALL_PER_S * 0.5, 6);
    p = peakStep(p, 0.2, 5000);
    expect(p.level).toBe(0.2);
    // A louder bar takes the marker with it at once; garbage reads as silence.
    expect(peakStep(p, 0.9, 10)).toEqual({ level: 0.9, heldMs: 0 });
    expect(peakStep({ level: 0, heldMs: 0 }, Number.NaN, 10).level).toBe(0);
    expect(peakStep({ level: 0, heldMs: 0 }, 7, 10).level).toBe(1);
  });
});
