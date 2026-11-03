import { describe, expect, it } from "vitest";
import type { RecordEditorView } from "./ipc";
import { SNAP_THRESHOLD_PX, snap, snapGuide, snapShift, snapTargets } from "./timeline-snap";

const view = (clips: [number, number][], programmeMs: number) =>
  ({ programmeMs, clips: clips.map(([progStartMs, progEndMs]) => ({ progStartMs, progEndMs })) }) as RecordEditorView;

describe("snap", () => {
  // 1 px = 10 ms.
  const px = 0.1;
  it("snaps within 8 px, not at 9 px", () => {
    expect(SNAP_THRESHOLD_PX).toBe(8);
    expect(snap(5_080, [5_000], px)).toEqual({ ms: 5_000, target: 5_000 });
    expect(snap(4_920, [5_000], px)).toEqual({ ms: 5_000, target: 5_000 });
    expect(snap(5_090, [5_000], px)).toEqual({ ms: 5_090, target: null });
  });
  it("takes the nearest target", () => {
    expect(snap(5_030, [5_000, 5_050], px)).toEqual({ ms: 5_050, target: 5_050 });
  });
  it("snaps nothing without targets or scale", () => {
    expect(snap(10, [], px)).toEqual({ ms: 10, target: null });
    expect(snap(10, [0], 0)).toEqual({ ms: 10, target: null });
  });
});

describe("snapTargets", () => {
  it("is 0, the programme end and every clip edge — sorted, once each", () => {
    expect(snapTargets(view([[0, 5_000], [5_000, 8_000]], 8_000))).toEqual([0, 5_000, 8_000]);
  });
  it("is not the playhead or another region (only what the view's clips name)", () => {
    const v = { ...view([[0, 3_000]], 3_000), zooms: [{ index: 0, progStartMs: 1_000, progEndMs: 2_000 }] } as RecordEditorView;
    expect(snapTargets(v)).toEqual([0, 3_000]);
  });
});

describe("snapShift", () => {
  it("moves the edge closest to a target onto it", () => {
    // start 1_040 is 4 px from 1_000; end 3_070 is 7 px from 3_000 → the start wins.
    expect(snapShift([1_040, 3_070], [1_000, 3_000], 0.1)).toEqual({ shiftMs: -40, target: 1_000 });
  });
  it("is zero when nothing is in range", () => {
    expect(snapShift([1_200], [1_000], 0.1)).toEqual({ shiftMs: 0, target: null });
  });
});

describe("snapGuide", () => {
  it("names the target an edge sits on", () => {
    expect(snapGuide([1_000, 2_500], [0, 2_500])).toBe(2_500);
    expect(snapGuide([1_000], [0, 2_500])).toBeNull();
  });
});
