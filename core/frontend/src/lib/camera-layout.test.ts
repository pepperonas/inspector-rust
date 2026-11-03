import { describe, expect, it } from "vitest";
import type { RecordCameraStyle, RecordEditDoc, RecordEditorView } from "./ipc";
import {
  ANCHOR_GRID,
  canShrink,
  effectiveRoundness,
  framingFromCrop,
  cropFromFraming,
  hasAnyCamera,
  layoutStartProg,
  moveLayoutRegion,
  passageAt,
  presetAt,
  regionAt,
  roundnessDefault,
  setLayoutRegionPreset,
  withMask,
  MIN_FRAMING_SIZE,
} from "./camera-layout";

const camera = (over: Partial<RecordCameraStyle> = {}): RecordCameraStyle => ({
  preset: "picture-in-picture",
  sizePct: 40,
  mask: "square",
  roundness: null,
  mirror: false,
  reactiveZoom: true,
  anchor: "bottom-right",
  crop: null,
  background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
  ...over,
});

const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 4,
    clips: [
      { startMs: 1_000, endMs: 5_000 },
      { startMs: 6_000, endMs: 10_000 },
    ],
    speed: [],
    crop: [null, null],
    zooms: [],
    annotations: [],
    audioTracks: [],
    camera: camera(),
    layoutRegions: [
      { id: "a", clip: 0, startMs: 2_000, preset: "dual-frame" },
      { id: "b", clip: 1, startMs: 8_000, preset: "camera-only" },
    ],
    ...over,
  }) as RecordEditDoc;

/** Clip 0 = programme 0–4 s, clip 1 = 4–8 s; regions as Rust maps them. */
const view = (d = doc()): Pick<RecordEditorView, "doc" | "layoutSpans" | "clips" | "clipCameras"> => ({
  doc: d,
  clips: [
    { progStartMs: 0, progEndMs: 4_000 },
    { progStartMs: 4_000, progEndMs: 8_000 },
  ],
  layoutSpans: [
    { index: 0, progStartMs: 1_000, progEndMs: 4_000 },
    { index: 1, progStartMs: 6_000, progEndMs: 8_000 },
  ],
  clipCameras: [true, false],
});

describe("camera layout", () => {
  it("roundness_resets_with_mask_until_touched", () => {
    expect(roundnessDefault("square")).toBe(0.7);
    expect(roundnessDefault("rectangle")).toBe(0.4);
    // Untouched (null) follows the mask.
    const c = withMask(camera(), "rectangle");
    expect(c.roundness).toBeNull();
    expect(effectiveRoundness(c)).toBe(0.4);
    // Set exactly to the old mask's standard still counts as untouched.
    expect(withMask(camera({ roundness: 0.7 }), "rectangle").roundness).toBeNull();
    // Tuned: stays.
    const tuned = withMask(camera({ roundness: 0.25 }), "rectangle");
    expect(tuned.roundness).toBe(0.25);
    expect(effectiveRoundness(tuned)).toBe(0.25);
  });

  it("shrink_toggle_only_for_pip", () => {
    expect(canShrink("picture-in-picture")).toBe(true);
    for (const p of ["dual-frame", "vertical-stack", "camera-only", "no-webcam"] as const) expect(canShrink(p)).toBe(false);
  });

  it("regionAt reads Rust's spans; presetAt falls back to the whole-video preset", () => {
    const v = view();
    expect(regionAt(v, 500)).toBeNull();
    expect(regionAt(v, 1_000)?.id).toBe("a");
    expect(regionAt(v, 3_999)?.id).toBe("a");
    expect(regionAt(v, 4_000)).toBeNull();
    expect(regionAt(v, 7_000)?.id).toBe("b");
    expect(presetAt(v, 500)).toBe("picture-in-picture");
    expect(presetAt(v, 2_000)).toBe("dual-frame");
    expect(passageAt(v, 2_000)).toEqual({ id: "a", progStartMs: 1_000, progEndMs: 4_000 });
    expect(passageAt(v, 500)).toBeNull();
  });

  it("hasAnyCamera comes from Rust's per-clip answer", () => {
    expect(hasAnyCamera(view())).toBe(true);
    expect(hasAnyCamera({ clipCameras: [false, false] })).toBe(false);
    expect(hasAnyCamera({})).toBe(false);
  });

  it("a region's preset changes and the region moves inside its clip", () => {
    const d = setLayoutRegionPreset(doc(), "a", "vertical-stack");
    expect(d.layoutRegions[0].preset).toBe("vertical-stack");
    expect(setLayoutRegionPreset(d, "nope", "dual-frame")).toBe(d);
    const m = moveLayoutRegion(doc(), "a", 500);
    expect(m.layoutRegions.find((r) => r.id === "a")?.startMs).toBe(1_000);
    expect(moveLayoutRegion(doc(), "a", 9_999).layoutRegions.find((r) => r.id === "a")?.startMs).toBe(4_999);
    // Moving onto another region's start replaces that one.
    const two = doc({ layoutRegions: [...doc().layoutRegions, { id: "c", clip: 0, startMs: 3_000, preset: "camera-only" }] });
    const onto = moveLayoutRegion(two, "a", 3_000);
    expect(onto.layoutRegions.map((r) => r.id).sort()).toEqual(["a", "b"]);
    const d0 = doc();
    expect(moveLayoutRegion(d0, "a", 2_000), "no move: no undo step").toBe(d0);
  });

  it("a dragged region start stays in its clip and snaps within 8 px", () => {
    const span = { progStartMs: 1_000, progEndMs: 4_000 };
    const clip = { progStartMs: 0, progEndMs: 4_000 };
    // 1 px = 10 ms.
    const snapping = { targets: [0, 4_000, 8_000], pxPerMs: 0.1 };
    expect(layoutStartProg(span, clip, -930, snapping)).toBe(0);
    expect(layoutStartProg(span, clip, -910, snapping)).toBe(90);
    expect(layoutStartProg(span, clip, -5_000)).toBe(0);
    expect(layoutStartProg(span, clip, 5_000)).toBe(3_999);
  });

  it("framing round-trips between crop and zoom + pan", () => {
    expect(cropFromFraming(1, { x: 0.5, y: 0.5 })).toBeNull();
    expect(cropFromFraming(0.5, { x: 1, y: 0 })).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.5 });
    expect(cropFromFraming(0.1, { x: 0, y: 0 })).toEqual({ x: 0, y: 0, w: MIN_FRAMING_SIZE, h: MIN_FRAMING_SIZE });
    expect(framingFromCrop(null)).toEqual({ size: 1, pan: { x: 0.5, y: 0.5 } });
    expect(framingFromCrop({ x: 0.25, y: 0.5, w: 0.5, h: 0.5 })).toEqual({ size: 0.5, pan: { x: 0.5, y: 1 } });
  });

  it("the anchor grid is 3×3 without the middle", () => {
    expect(ANCHOR_GRID.flat()).toEqual(["top-left", "top", "top-right", "left", null, "right", "bottom-left", "bottom", "bottom-right"]);
  });
});
