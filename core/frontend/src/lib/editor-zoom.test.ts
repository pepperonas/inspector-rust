import { describe, expect, it } from "vitest";
import EDITOR_RS from "../../../rust-lib/src/record/editor.rs?raw";
import {
  INSPECTOR_RANGES,
  MIN_ZOOM_MS,
  ZOOM_DEPTHS,
  clickVolume,
  dueClickHits,
  fmtZoom,
  focusFromPoint,
  moveZoomProg,
  trimZoomProg,
  zoomScale,
  zoomSourceAt,
} from "./editor";

const clip = { progStartMs: 1_000, progEndMs: 6_000 };
const span = { progStartMs: 2_000, progEndMs: 4_000 };

describe("zoom track geometry", () => {
  it("moves a block and keeps its length", () => {
    expect(moveZoomProg(span, clip, 500)).toEqual({ progStartMs: 2_500, progEndMs: 4_500 });
  });

  it("clamps a drag past either clip edge", () => {
    expect(moveZoomProg(span, clip, 99_000)).toEqual({ progStartMs: 4_000, progEndMs: 6_000 });
    expect(moveZoomProg(span, clip, -99_000)).toEqual({ progStartMs: 1_000, progEndMs: 3_000 });
  });

  it("trims inside the clip and never under the minimum", () => {
    expect(trimZoomProg(span, clip, "start", -99_000)).toEqual({ progStartMs: 1_000, progEndMs: 4_000 });
    expect(trimZoomProg(span, clip, "end", 99_000)).toEqual({ progStartMs: 2_000, progEndMs: 6_000 });
    expect(trimZoomProg(span, clip, "start", 99_000)).toEqual({ progStartMs: 4_000 - MIN_ZOOM_MS, progEndMs: 4_000 });
    expect(trimZoomProg(span, clip, "end", -99_000)).toEqual({ progStartMs: 2_000, progEndMs: 2_000 + MIN_ZOOM_MS });
  });

  it("maps programme to source through Rust, the clip edges by the clip itself", async () => {
    const calls: number[] = [];
    const toSource = async (p: number) => {
      calls.push(p);
      return { clip: 0, sourceMs: p + 300 };
    };
    const bounds = { startMs: 500, endMs: 5_500 };
    expect(await zoomSourceAt(0, clip, bounds, 3_000, toSource)).toBe(3_300);
    expect(await zoomSourceAt(0, clip, bounds, 6_000, toSource)).toBe(5_500);
    expect(await zoomSourceAt(0, clip, bounds, 1_000, toSource)).toBe(500);
    expect(calls).toEqual([3_000]);
    // Rust names another clip: the edge, never a foreign source time.
    expect(await zoomSourceAt(0, clip, bounds, 3_000, async () => ({ clip: 1, sourceMs: 9 }))).toBe(5_500);
  });
});

describe("zoom scale", () => {
  it("labels depth factors and honours a custom scale", () => {
    expect(fmtZoom(1.25)).toBe("1.25×");
    expect(fmtZoom(5)).toBe("5×");
    expect(zoomScale({ depth: 3, customScale: null })).toBe(ZOOM_DEPTHS[2]);
    expect(zoomScale({ depth: 3, customScale: 2.5 })).toBe(2.5);
    expect(zoomScale({ depth: 3, customScale: 9 })).toBe(5);
  });

  it("the custom-scale slider spans Rust's clamp", () => {
    const min = Number(/MIN_ZOOM_SCALE: f64 = ([\d.]+)/.exec(EDITOR_RS)?.[1]);
    const max = Number(/MAX_ZOOM_SCALE: f64 = ([\d.]+)/.exec(EDITOR_RS)?.[1]);
    expect([INSPECTOR_RANGES.zoomScale.min, INSPECTOR_RANGES.zoomScale.max]).toEqual([min, max]);
  });
});

describe("click sound preview", () => {
  const hits = [
    { progMs: 1_000, gain: 1, release: false },
    { progMs: 1_090, gain: 0.7, release: true },
    { progMs: 2_000, gain: 1, release: false },
  ];

  it("plays hits once, inside the passed window", () => {
    expect(dueClickHits(hits, 900, 1_000).map((h) => h.progMs)).toEqual([1_000]);
    expect(dueClickHits(hits, 1_000, 1_100).map((h) => h.progMs)).toEqual([1_090]);
    expect(dueClickHits(hits, 1_100, 1_900)).toEqual([]);
  });

  it("applies the user's dB like the export and never exceeds 1", () => {
    expect(clickVolume(1, 0)).toBe(1);
    expect(clickVolume(0.7, -6)).toBeCloseTo(0.7 * 10 ** (-6 / 20), 6);
    expect(clickVolume(2, 6)).toBe(1);
  });
});

describe("focus from a click on the preview", () => {
  it("maps into the screen box and clamps outside it", () => {
    const rect = [0.1, 0.2, 0.8, 0.6] as const;
    expect(focusFromPoint(rect, 0.5, 0.5)).toEqual({ x: 0.5, y: 0.5 });
    expect(focusFromPoint(rect, 0.1, 0.2)).toEqual({ x: 0, y: 0 });
    expect(focusFromPoint(rect, 0, 1)).toEqual({ x: 0, y: 1 });
  });
});
