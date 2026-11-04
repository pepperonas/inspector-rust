import { describe, expect, it, vi } from "vitest";
import EDITOR_RS from "../../../rust-lib/src/record/editor.rs?raw";
import CAPTIONS_RS from "../../../rust-lib/src/record/captions.rs?raw";
import type { RecordEditDoc, RecordEditorView } from "./ipc";
import {
  ANNOTATION_DEFAULT_RECT,
  CAPTION_DEFAULTS,
  CAPTION_FONT_SIZE_RANGE,
  CAPTION_FONT_SIZE_UI_RANGE,
  CAPTION_INSET_STEP,
  CAPTION_INSET_X_LANDSCAPE,
  CAPTION_INSET_X_MAX,
  CAPTION_INSET_X_PORTRAIT,
  CAPTION_INSET_Y_LANDSCAPE,
  CAPTION_INSET_Y_MAX,
  CAPTION_INSET_Y_PORTRAIT,
  CAPTION_LANDSCAPE_ASPECT,
  CAPTION_LOOKS,
  CAPTION_PLATE_DARK,
  CAPTION_PLATE_LIGHT,
  CAPTION_PLATE_OFF_BELOW,
  CAPTION_PLATE_OPACITY_RANGE,
  CAPTION_WORDS_RANGE,
  captionInsets,
  BLUR_BLOCK_RANGE,
  BLUR_INTENSITY_RANGE,
  DEFAULT_ARROW_COLOR,
  DEFAULT_FREEHAND_POINTS,
  DEFAULT_TEXT_PLATE,
  FADE_MAX_MS,
  FONT_SIZE_RANGE,
  MUSIC_DEFAULT_FADE_MS,
  MUSIC_DEFAULT_GAIN_DB,
  SYSTEM_GAIN_RANGE,
  TEXT_FONTS,
  TRACK_GAIN_RANGE,
  addAnnotation,
  addLayoutRegion,
  addTrack,
  CAMERA_ROUNDNESS_RECTANGLE,
  CAMERA_ROUNDNESS_SQUARE,
  CAMERA_SIZE_RANGE,
  cameraRoundness,
  DEFAULT_CAMERA_BLUR,
  DEFAULT_CAMERA_SIZE,
  LAYOUT_TRANSITION_SEC,
  removeLayoutRegion,
  setCamera,
  removeAnnotation,
  removeTrack,
  setSystemGain,
  updateAnnotation,
  updateTrack,
  History,
  ZOOM_DEPTHS,
  addZoom,
  removeAutoZooms,
  removeZoom,
  setAppearance,
  setCursorStyle,
  updateZoom,
  HISTORY_MAX,
  addSpeed,
  clipAt,
  clipDropGap,
  clipMedia,
  clipMoveTarget,
  cropForPreset,
  deleteClip,
  fitPreview,
  formatTimecode,
  setCrop,
  setTrim,
  trimSourceAt,
  splitAt,
  cutSourceRange,
  setZoomRotation,
  addEffect,
  setEffectValue,
  moveEffect,
  deleteEffect,
  canRestoreGap,
  restoreGap,
} from "./editor";

const base = (over: Partial<RecordEditDoc> = {}): RecordEditDoc => ({
  version: 3,
  clips: [{ startMs: 0, endMs: 10_000 }],
  speed: [],
  crop: [null],
  output: { aspect: "source" },
  micGainDb: 0,
  appearance: {
    background: { kind: "color", color: "#000000" },
    backgroundMotion: "none",
    padding: 0,
    radiusPx: 0,
    shadow: 0,
    backgroundBlur: 0,
    motionBlur: 0,
    frame: "none",
    frameTheme: "light",
    depthOfField: true,
  },
  cursor: {
    show: false,
    autoHide: false,
    theme: "default",
    custom: null,
    size: 3.6,
    smoothing: 0.3,
    motionBlur: 0.35,
    clickBounce: 1,
    clickImpact: false,
    clickSound: false,
    clickSoundGainDb: 0,
    model3d: false,
  },
  zooms: [],
  autoZoomDone: true,
  annotations: [],
  audioTracks: [],
  systemGainDb: 0,
  camera: {
    preset: "no-webcam",
    sizePct: 40,
    mask: "square",
    roundness: null,
    mirror: false,
    reactiveZoom: true,
    anchor: "bottom-right",
    crop: null,
    background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
  },
  layoutRegions: [],
  transcripts: [],
  captions: CAPTION_DEFAULTS,
  ...over,
});

describe("splitAt", () => {
  it("splits at the source position Rust returned for a time inside a 2× region", () => {
    // Clip 0–10 s, 2× on source 2–6 s. Rust maps programme 3000 ms → source 4000 ms.
    const doc = base({ speed: [{ clip: 0, startMs: 2000, endMs: 6000, speed: 2 }] });
    const out = splitAt(doc, 0, 4000);
    expect(out.clips).toEqual([
      { startMs: 0, endMs: 4000 },
      { startMs: 4000, endMs: 10_000 },
    ]);
    // The region is cut exactly there and follows its halves.
    expect(out.speed).toEqual([
      { clip: 0, startMs: 2000, endMs: 4000, speed: 2 },
      { clip: 1, startMs: 4000, endMs: 6000, speed: 2 },
    ]);
    expect(out.crop).toEqual([null, null]);
  });

  it("re-indexes regions of later clips and copies the crop", () => {
    const crop = { x: 0.1, y: 0.1, w: 0.5, h: 0.5 };
    const doc = base({
      clips: [{ startMs: 0, endMs: 4000 }, { startMs: 5000, endMs: 9000 }],
      speed: [{ clip: 1, startMs: 5000, endMs: 6000, speed: 4 }],
      crop: [crop, null],
    });
    const out = splitAt(doc, 0, 2000);
    expect(out.clips.length).toBe(3);
    expect(out.speed).toEqual([{ clip: 2, startMs: 5000, endMs: 6000, speed: 4 }]);
    expect(out.crop).toEqual([crop, crop, null]);
  });

  it("refuses a split that leaves a piece under 100 ms", () => {
    const doc = base();
    expect(splitAt(doc, 0, 50)).toBe(doc);
    expect(splitAt(doc, 0, 9_950)).toBe(doc);
    expect(splitAt(doc, 3, 5000)).toBe(doc);
  });
});

describe("deleteClip / setTrim / addSpeed / setCrop", () => {
  it("never deletes the last clip", () => {
    const doc = base();
    expect(deleteClip(doc, 0)).toBe(doc);
  });

  it("deletes a clip with its regions and crop, later regions move down", () => {
    const doc = base({
      clips: [{ startMs: 0, endMs: 4000 }, { startMs: 5000, endMs: 9000 }],
      speed: [
        { clip: 0, startMs: 1000, endMs: 2000, speed: 2 },
        { clip: 1, startMs: 6000, endMs: 7000, speed: 3 },
      ],
      crop: [null, { x: 0, y: 0, w: 0.5, h: 0.5 }],
    });
    const out = deleteClip(doc, 0);
    expect(out.clips).toEqual([{ startMs: 5000, endMs: 9000 }]);
    expect(out.speed).toEqual([{ clip: 0, startMs: 6000, endMs: 7000, speed: 3 }]);
    expect(out.crop).toEqual([{ x: 0, y: 0, w: 0.5, h: 0.5 }]);
  });

  it("setTrim keeps start before end", () => {
    const out = setTrim(base(), 0, 3000, 2000);
    expect(out.clips[0]).toEqual({ startMs: 2000, endMs: 3000 });
    expect(setTrim(base(), 0, -5, 8000).clips[0]).toEqual({ startMs: 0, endMs: 8000 });
  });

  it("addSpeed appends (later wins in Rust) and clamps the value", () => {
    const out = addSpeed(base(), 0, 1000, 2000, 99);
    expect(out.speed).toEqual([{ clip: 0, startMs: 1000, endMs: 2000, speed: 16 }]);
    expect(addSpeed(base(), 0, 2000, 1000, 0.01).speed[0]).toEqual({ clip: 0, startMs: 1000, endMs: 2000, speed: 0.25 });
  });

  it("setCrop sets and clears one clip", () => {
    const r = { x: 0.1, y: 0.2, w: 0.3, h: 0.4 };
    const set = setCrop(base(), 0, r);
    expect(set.crop).toEqual([r]);
    expect(setCrop(set, 0, null).crop).toEqual([null]);
  });
});

describe("cropForPreset", () => {
  it("16:9 in 3024×1964 uses the full width, centred", () => {
    const r = cropForPreset("16:9", 3024, 1964);
    expect(r.x).toBe(0);
    expect(r.w).toBe(1);
    expect(r.h).toBeCloseTo(3024 / (16 / 9) / 1964, 6);
    expect(r.y).toBeCloseTo((1 - r.h) / 2, 6);
  });

  it("9:16 uses the full height", () => {
    const r = cropForPreset("9:16", 3024, 1964);
    expect(r.h).toBe(1);
    expect(r.y).toBe(0);
    expect(r.w).toBeCloseTo((1964 * 9) / 16 / 3024, 6);
  });

  it("source is the whole frame", () => {
    expect(cropForPreset("source", 3024, 1964)).toEqual({ x: 0, y: 0, w: 1, h: 1 });
  });
});

describe("History", () => {
  it("undo/redo walk the stack and a new push drops the redo branch", () => {
    const h = new History(base());
    const a = base({ micGainDb: -3 });
    const b = base({ micGainDb: -6 });
    h.push(a);
    h.push(b);
    expect(h.canUndo()).toBe(true);
    expect(h.undo()).toBe(a);
    expect(h.canRedo()).toBe(true);
    const c = base({ micGainDb: 6 });
    h.push(c);
    expect(h.canRedo()).toBe(false);
    expect(h.redo()).toBeNull();
    expect(h.undo()).toBe(a);
  });

  it("keeps at most 50 undo steps", () => {
    const h = new History(base());
    for (let i = 0; i < 80; i++) h.push(base({ micGainDb: -i }));
    let n = 0;
    while (h.undo()) n++;
    expect(n).toBe(HISTORY_MAX);
    expect(HISTORY_MAX).toBe(50);
  });
});

describe("formatTimecode", () => {
  it("formats minutes, seconds and hundredths", () => {
    expect(formatTimecode(0)).toBe("0:00.00");
    expect(formatTimecode(5_200)).toBe("0:05.20");
    expect(formatTimecode(65_432)).toBe("1:05.43");
    expect(formatTimecode(3_725_000)).toBe("1:02:05.00");
    expect(formatTimecode(-10)).toBe("0:00.00");
  });
});

const view: RecordEditorView = {
  doc: base({
    clips: [{ startMs: 0, endMs: 10_000 }, { startMs: 12_000, endMs: 14_000 }],
    speed: [{ clip: 0, startMs: 2000, endMs: 6000, speed: 2 }],
    crop: [null, null],
  }),
  outputSize: [1920, 1080],
  programmeMs: 10_000,
  clips: [{ progStartMs: 0, progEndMs: 8000 }, { progStartMs: 8000, progEndMs: 10_000 }],
  speed: [{ index: 0, progStartMs: 2000, progEndMs: 4000 }],
  zooms: [],
  clickHits: [],
  screenRects: [],
};

describe("clipAt (from Rust spans)", () => {
  it("finds the clip span under a programme time", () => {
    expect(clipAt(view, 0)).toBe(0);
    expect(clipAt(view, 8000)).toBe(1);
    expect(clipAt(view, 10_000)).toBe(1);
  });
});

describe("trimSourceAt", () => {
  const span = { progStartMs: 0, progEndMs: 8000 };
  const bounds = { startMs: 0, endMs: 10_000 };
  it("asks Rust inside the clip (integer programme ms)", async () => {
    const toSource = vi.fn(async (p: number) => ({ clip: 0, sourceMs: p === 3000 ? 4000 : -1 }));
    expect(await trimSourceAt(0, "end", span, bounds, 2999.7, toSource)).toBe(4000);
    expect(toSource).toHaveBeenCalledWith(3000);
  });
  it("past the span the uncovered source runs at 1×, without asking Rust", async () => {
    const toSource = vi.fn(async () => null);
    expect(await trimSourceAt(0, "end", span, bounds, 8500, toSource)).toBe(10_500);
    expect(await trimSourceAt(0, "start", { progStartMs: 1000, progEndMs: 5000 }, { startMs: 3000, endMs: 7000 }, 400, toSource)).toBe(2400);
    expect(await trimSourceAt(0, "start", span, { startMs: 200, endMs: 5000 }, -900, toSource)).toBe(0);
    expect(toSource).not.toHaveBeenCalled();
  });
  it("an answer in another clip keeps the edge", async () => {
    const toSource = async () => ({ clip: 1, sourceMs: 12_000 });
    expect(await trimSourceAt(0, "end", span, bounds, 4000, toSource)).toBe(10_000);
  });
});

describe("fitPreview", () => {
  it("fits the output aspect into the panel, capped", () => {
    expect(fitPreview(800, 800, 16 / 9, 1280, 720)).toEqual({ w: 800, h: 450 });
    expect(fitPreview(4000, 3000, 16 / 9, 1280, 720)).toEqual({ w: 1280, h: 720 });
    expect(fitPreview(0, 0, 1, 1280, 720)).toEqual({ w: 16, h: 16 });
  });
});

describe("zoom regions", () => {
  it("addZoom takes 2 s from the playhead and clamps at the clip end", () => {
    const doc = base();
    const a = addZoom(doc, 0, 3000);
    expect(a.zooms).toHaveLength(1);
    expect(a.zooms[0]).toMatchObject({
      clip: 0, startMs: 3000, endMs: 5000, depth: 3, focusX: 0.5, focusY: 0.5,
      focusMode: "auto", origin: "manual", customScale: null, hideCursor: false,
    });
    expect(a.zooms[0].id).toMatch(/[0-9a-f-]{36}/);
    const b = addZoom(doc, 0, 9000);
    expect([b.zooms[0].startMs, b.zooms[0].endMs]).toEqual([9000, 10_000]);
    expect(addZoom(doc, 5, 0)).toBe(doc);
  });

  it("removeAutoZooms keeps the manual ones", () => {
    let doc = addZoom(base(), 0, 1000);
    doc = addZoom(doc, 0, 5000);
    doc = updateZoom(doc, doc.zooms[0].id, { origin: "auto" });
    const out = removeAutoZooms(doc);
    expect(out.zooms.map((z) => z.startMs)).toEqual([5000]);
    expect(removeZoom(out, out.zooms[0].id).zooms).toEqual([]);
  });

  it("splitAt puts a spanning zoom into both halves; deleteClip re-indexes", () => {
    let doc = addZoom(base(), 0, 1000); // 1000–3000
    doc = addZoom(doc, 0, 6000); // 6000–8000
    const out = splitAt(doc, 0, 2000);
    expect(out.zooms.map((z) => [z.clip, z.startMs, z.endMs])).toEqual([
      [0, 1000, 2000],
      [1, 2000, 3000],
      [1, 6000, 8000],
    ]);
    const del = deleteClip(out, 0);
    expect(del.zooms.map((z) => [z.clip, z.startMs])).toEqual([[0, 2000], [0, 6000]]);
  });

  it("setAppearance / setCursorStyle merge a patch", () => {
    const doc = setCursorStyle(setAppearance(base(), { padding: 50 }), { show: true });
    expect(doc.appearance.padding).toBe(50);
    expect(doc.appearance.frame).toBe("none");
    expect(doc.cursor.show).toBe(true);
    expect(doc.cursor.size).toBe(3.6);
  });

  it("ZOOM_DEPTHS mirrors the constant in record/editor.rs", () => {
    const m = /pub const ZOOM_DEPTHS: \[f64; 6\] = \[([^\]]+)\];/.exec(EDITOR_RS);
    expect(m).not.toBeNull();
    const rust = m![1].split(",").map((v) => Number(v.trim()));
    expect([...ZOOM_DEPTHS]).toEqual(rust);
  });
});

describe("annotations + audio tracks (v3)", () => {
  it("addAnnotation: upstream box, 2 s clamped into the clip, zIndex above the rest", () => {
    let doc = base({ clips: [{ startMs: 0, endMs: 5_000 }, { startMs: 6_000, endMs: 7_000 }], crop: [null, null] });
    doc = addAnnotation(doc, "text", 0, 1_000);
    const a = doc.annotations[0];
    expect([a.x, a.y, a.w, a.h]).toEqual([50, 50, 30, 20]);
    expect([a.clip, a.startMs, a.endMs, a.zIndex]).toEqual([0, 1_000, 3_000, 1]);
    expect(a.body).toMatchObject({
      kind: "text",
      color: "#ffffff",
      background: "rgba(0, 0, 0, 0.7)",
      fontSize: 32,
      fontFamily: "Inter",
      bold: true,
      align: "center",
      animation: "none",
    });
    doc = updateAnnotation(doc, a.id, { zIndex: 7 });
    doc = addAnnotation(doc, "arrow", 1, 6_500);
    const b = doc.annotations[1];
    expect([b.startMs, b.endMs, b.zIndex]).toEqual([6_000, 7_000, 8]); // shifted back to keep its length
    expect(b.body).toEqual({ kind: "arrow", direction: "right", color: "#34B27B", strokeWidth: 4 });
    expect(b.id).not.toBe(a.id);
    doc = addAnnotation(doc, "blur", 0, 0);
    expect(doc.annotations[2].body).toMatchObject({ kind: "blur", style: "mosaic", shape: "rectangle", color: "white", intensity: 12, blockSize: 12 });
    expect(addAnnotation(doc, "image", 9, 0)).toBe(doc);
  });

  it("addAnnotation at the clip end keeps its length by starting earlier", () => {
    const doc = addAnnotation(base(), "text", 0, 10_000);
    expect([doc.annotations[0].startMs, doc.annotations[0].endMs]).toEqual([8_000, 10_000]);
    const short = addAnnotation(base({ clips: [{ startMs: 0, endMs: 500 }] }), "text", 0, 500);
    expect([short.annotations[0].startMs, short.annotations[0].endMs]).toEqual([0, 500]);
  });

  it("updateAnnotation merges a body patch and keeps the rest", () => {
    let doc = addAnnotation(base(), "text", 0, 0);
    const id = doc.annotations[0].id;
    doc = updateAnnotation(doc, id, { x: 10, body: { content: "Hallo" } });
    expect(doc.annotations[0].x).toBe(10);
    expect(doc.annotations[0].body).toMatchObject({ kind: "text", content: "Hallo", fontSize: 32 });
    expect(updateAnnotation(doc, "nope", { x: 1 })).toBe(doc);
    // @ts-expect-error the kind is not patchable
    expect(updateAnnotation(doc, id, { body: { kind: "arrow" } }).annotations[0].body.kind).toBe("text");
    expect(removeAnnotation(doc, id).annotations).toEqual([]);
  });

  it("addTrack: music −18 dB with 1 s fades, voiceover 0 dB with its span", () => {
    let doc = addTrack(base(), "music", "song.mp3", 30_000, 0, 2_000);
    const m = doc.audioTracks[0];
    expect(m).toMatchObject({ kind: "music", file: "song.mp3", clip: 0, startMs: 2_000, durationMs: 30_000, offsetMs: 0 });
    expect([m.gainDb, m.fadeInMs, m.fadeOutMs, m.spanMs, m.looped, m.muted]).toEqual([-18, 1_000, 1_000, null, false, false]);
    doc = addTrack(doc, "voiceover", "vo.wav", 4_200, 0, 99_000);
    const v = doc.audioTracks[1];
    expect([v.gainDb, v.fadeInMs, v.fadeOutMs, v.spanMs, v.startMs]).toEqual([0, 0, 0, 4_200, 10_000]);
    doc = updateTrack(doc, m.id, { looped: true, gainDb: -6 });
    expect(doc.audioTracks[0]).toMatchObject({ looped: true, gainDb: -6, file: "song.mp3" });
    expect(removeTrack(doc, v.id).audioTracks.map((t) => t.id)).toEqual([m.id]);
  });

  it("setSystemGain clamps to ±12 dB", () => {
    expect(setSystemGain(base(), 20).systemGainDb).toBe(12);
    expect(setSystemGain(base(), -40).systemGainDb).toBe(-12);
    expect(setSystemGain(base(), -3.5).systemGainDb).toBe(-3.5);
    expect(setSystemGain(base(), Number.NaN).systemGainDb).toBe(0);
  });

  it("split and delete carry annotations, tracks and the clip source", () => {
    let doc = base({ clips: [{ startMs: 0, endMs: 10_000, source: "take-2" }] });
    doc = addAnnotation(doc, "text", 0, 1_000); // 1–3 s, cut at 2 s
    doc = addAnnotation(doc, "text", 0, 6_000); // 6–8 s, all in the second half
    doc = addTrack(doc, "music", "a.mp3", 9_000, 0, 500);
    doc = addTrack(doc, "music", "b.mp3", 9_000, 0, 5_000);
    const split = splitAt(doc, 0, 2_000);
    expect(split.clips.map((c) => c.source)).toEqual(["take-2", "take-2"]);
    expect(split.annotations.map((a) => [a.clip, a.startMs, a.endMs])).toEqual([
      [0, 1_000, 2_000],
      [1, 2_000, 3_000],
      [1, 6_000, 8_000],
    ]);
    expect(split.audioTracks.map((t) => t.clip)).toEqual([0, 1]);
    const del = deleteClip(split, 0);
    expect(del.annotations.map((a) => a.clip)).toEqual([0, 0]);
    expect(del.audioTracks.map((t) => [t.clip, t.file])).toEqual([[0, "b.mp3"]]);
    expect(setTrim(split, 1, 2_500, 9_000).clips[1].source).toBe("take-2");
  });

  it("the stage-4 constants mirror record/editor.rs", () => {
    const num = (name: string) => {
      const m = new RegExp(`pub const ${name}: [^=]+= ([^;]+);`).exec(EDITOR_RS);
      expect(m, name).not.toBeNull();
      return m![1];
    };
    const tuple = (name: string) => num(name).replace(/[()]/g, "").split(",").map((v) => Number(v.trim()));
    const str = (name: string) => JSON.parse(num(name));
    expect(JSON.parse(num("TEXT_FONTS"))).toEqual([...TEXT_FONTS]);
    expect(tuple("ANNOTATION_DEFAULT_RECT")).toEqual([...ANNOTATION_DEFAULT_RECT]);
    expect(tuple("FONT_SIZE_RANGE")).toEqual([...FONT_SIZE_RANGE]);
    expect(tuple("BLUR_INTENSITY_RANGE")).toEqual([...BLUR_INTENSITY_RANGE]);
    expect(tuple("BLUR_BLOCK_RANGE")).toEqual([...BLUR_BLOCK_RANGE]);
    expect(tuple("TRACK_GAIN_RANGE")).toEqual([...TRACK_GAIN_RANGE]);
    expect(tuple("SYSTEM_GAIN_RANGE")).toEqual([...SYSTEM_GAIN_RANGE]);
    expect(Number(num("FADE_MAX_MS"))).toBe(FADE_MAX_MS);
    expect(Number(num("MUSIC_DEFAULT_GAIN_DB"))).toBe(MUSIC_DEFAULT_GAIN_DB);
    expect(Number(num("MUSIC_DEFAULT_FADE_MS"))).toBe(MUSIC_DEFAULT_FADE_MS);
    expect(str("DEFAULT_TEXT_PLATE")).toBe(DEFAULT_TEXT_PLATE);
    expect(str("DEFAULT_ARROW_COLOR")).toBe(DEFAULT_ARROW_COLOR);
    const pts = /pub const DEFAULT_FREEHAND_POINTS: [^=]+= \[([\s\S]*?)\];/.exec(EDITOR_RS);
    expect(pts).not.toBeNull();
    const rust = [...pts![1].matchAll(/\(([\d.]+), ([\d.]+)\)/g)].map((m) => [Number(m[1]), Number(m[2])]);
    expect(rust).toHaveLength(9);
    expect(rust).toEqual(DEFAULT_FREEHAND_POINTS.map(([x, y]) => [x, y]));
  });
});

describe("clip reorder targets", () => {
  const spans = [
    { progStartMs: 0, progEndMs: 3_000 },
    { progStartMs: 3_000, progEndMs: 6_000 },
    { progStartMs: 6_000, progEndMs: 10_000 },
  ];
  it("the gap follows the clip middles", () => {
    expect(clipDropGap(spans, 0)).toBe(0);
    expect(clipDropGap(spans, 1_499)).toBe(0);
    expect(clipDropGap(spans, 1_500)).toBe(1);
    expect(clipDropGap(spans, 4_600)).toBe(2);
    expect(clipDropGap(spans, 9_999)).toBe(3);
  });
  it("a gap becomes the remove-then-insert index; its own gaps are no move", () => {
    expect(clipMoveTarget(0, 3, 3)).toBe(2);
    expect(clipMoveTarget(0, 2, 3)).toBe(1);
    expect(clipMoveTarget(2, 0, 3)).toBe(0);
    expect(clipMoveTarget(2, 1, 3)).toBe(1);
    expect(clipMoveTarget(1, 1, 3)).toBeNull();
    expect(clipMoveTarget(1, 2, 3)).toBeNull();
    expect(clipMoveTarget(5, 0, 3)).toBeNull();
    expect(clipMoveTarget(0, 4, 3)).toBeNull();
  });
});

describe("clipMedia", () => {
  const main = { width: 1920, height: 1080, systemAudio: true, micOffsetMs: 40 };
  const doc = { clips: [{ startMs: 0, endMs: 1 }, { startMs: 0, endMs: 1, source: "t1" }, { startMs: 0, endMs: 1, source: "gone" }] } as unknown as RecordEditDoc;
  const takes = { t1: { durationMs: 1, width: 640, height: 480, systemAudio: false, hasMic: true, micOffsetMs: -20, hasCursor: false } };
  it("names the clip's own recording, size and audio", () => {
    expect(clipMedia(doc, takes, 0, main)).toMatchObject({ dir: "", width: 1920, height: 1080, systemAudio: true, hasMic: true, micOffsetMs: 40, missing: false });
    expect(clipMedia(doc, takes, 1, main)).toMatchObject({ dir: "takes/t1", width: 640, height: 480, systemAudio: false, hasMic: true, micOffsetMs: -20, missing: false });
    expect(clipMedia(doc, takes, 2, main)).toMatchObject({ missing: true, systemAudio: false, hasMic: false });
  });
});


describe("annotation editing helpers (stage 4, Task 6)", () => {
  it("dragAnnotationBox: a non-moving drag keeps the box; sizes never go under 1 %", async () => {
    const { dragAnnotationBox } = await import("./editor");
    const b = { x: 10, y: 20, w: 30, h: 40 };
    expect(dragAnnotationBox(b, "move", 0, 0)).toEqual(b);
    expect(dragAnnotationBox(b, "w", 500, 0)).toEqual({ x: 39, y: 20, w: 1, h: 40 });
    expect(dragAnnotationBox(b, "n", 0, -500)).toEqual({ x: 10, y: 0, w: 30, h: 60 });
    // A box bigger than the anchor (hand-edited) still moves inside 0…100.
    expect(dragAnnotationBox({ x: 0, y: 0, w: 100, h: 100 }, "move", 30, 30)).toEqual({ x: 0, y: 0, w: 100, h: 100 });
  });

  it("nextAnnotationId ignores spans whose annotation is gone; ties keep list order", async () => {
    const { nextAnnotationId } = await import("./editor");
    const doc = { annotations: [{ id: "a" }, { id: "b" }] } as unknown as Pick<RecordEditDoc, "annotations">;
    const spans = [
      { index: 1, progStartMs: 0 },
      { index: 0, progStartMs: 0 },
      { index: 7, progStartMs: 0 },
    ];
    expect(nextAnnotationId(doc, spans, null, false)).toBe("a");
    expect(nextAnnotationId(doc, spans, "a", false)).toBe("b");
    expect(nextAnnotationId(doc, spans, "b", false)).toBe("a");
    expect(nextAnnotationId(doc, spans, null, true)).toBe("b");
    expect(nextAnnotationId({ annotations: [] }, [], null, false)).toBeNull();
  });

  it("freehand points: insert into the nearest edge, move clamped, remove keeps three", async () => {
    const { insertFreehandPoint, moveFreehandPoint, removeFreehandPoint } = await import("./editor");
    const tri: [number, number][] = [
      [0, 0],
      [100, 0],
      [50, 100],
    ];
    // Nearest to the closing edge 50,100 → 0,0: inserted at the end.
    expect(insertFreehandPoint(tri, [20, 50])).toEqual([[0, 0], [100, 0], [50, 100], [20, 50]]);
    expect(insertFreehandPoint(tri, [50, -10])).toEqual([[0, 0], [50, 0], [100, 0], [50, 100]]);
    expect(moveFreehandPoint(tri, 1, [150, -3])).toEqual([[0, 0], [100, 0], [50, 100]]);
    expect(moveFreehandPoint(tri, 2, [40, 60])).toEqual([[0, 0], [100, 0], [40, 60]]);
    expect(removeFreehandPoint(tri, 0)).toEqual(tri);
    expect(removeFreehandPoint([...tri, [0, 50]], 3)).toEqual(tri);
    // The input is never mutated.
    expect(tri).toHaveLength(3);
  });

  it("annotationOutputRect: frame space for text/image/arrow, the screen box for a blur", async () => {
    const { annotationOutputRect, defaultAnnotationBody } = await import("./editor");
    const box = { x: 50, y: 50, w: 20, h: 10 };
    expect(annotationOutputRect({ ...box, body: defaultAnnotationBody("text") }, [0.25, 0, 0.5, 1])).toEqual([0.5, 0.5, 0.2, 0.1]);
    expect(annotationOutputRect({ ...box, body: defaultAnnotationBody("blur") }, [0.25, 0, 0.5, 1])).toEqual([0.5, 0.5, 0.1, 0.1]);
    expect(annotationOutputRect({ ...box, body: defaultAnnotationBody("blur") }, null)).toEqual([0.5, 0.5, 0.2, 0.1]);
  });

  it("zoomActiveAt reads Rust's zoom spans of the same clip only", async () => {
    const { zoomActiveAt } = await import("./editor");
    const view = {
      doc: { zooms: [{ clip: 0 }, { clip: 1 }] },
      zooms: [
        { index: 0, progStartMs: 1_000, progEndMs: 2_000 },
        { index: 1, progStartMs: 5_000, progEndMs: 6_000 },
      ],
    } as unknown as Pick<RecordEditorView, "doc" | "zooms">;
    expect(zoomActiveAt(view, 0, 1_500)).toBe(true);
    expect(zoomActiveAt(view, 0, 2_000)).toBe(false);
    expect(zoomActiveAt(view, 0, 5_500)).toBe(false);
    expect(zoomActiveAt(view, 1, 5_500)).toBe(true);
  });

  it("laneRows stacks overlaps greedily and caps the rows", async () => {
    const { laneRows } = await import("./editor");
    const s = (index: number, a: number, b: number) => ({ index, progStartMs: a, progEndMs: b });
    const rows = laneRows([s(0, 0, 10), s(1, 2, 4), s(2, 3, 5), s(3, 4, 6), s(4, 10, 12)], 3);
    expect([0, 1, 2, 3, 4].map((i) => rows.get(i))).toEqual([0, 1, 2, 1, 0]);
    // A fourth simultaneous one shares the last row.
    expect(laneRows([s(0, 0, 9), s(1, 0, 9), s(2, 0, 9), s(3, 0, 9)], 3).get(3)).toBe(2);
  });

  it("annotationShownAt reads Rust's annotation spans, end exclusive", async () => {
    const { annotationShownAt } = await import("./editor");
    const view = {
      doc: { annotations: [{ id: "a" }, { id: "b" }] },
      annotationSpans: [{ index: 1, progStartMs: 1_000, progEndMs: 2_000 }],
    } as unknown as Pick<RecordEditorView, "doc" | "annotationSpans">;
    expect(annotationShownAt(view, "b", 1_000)).toBe(true);
    expect(annotationShownAt(view, "b", 2_000)).toBe(false);
    expect(annotationShownAt(view, "a", 1_500)).toBe(false);
    expect(annotationShownAt({ ...view, annotationSpans: undefined }, "b", 1_500)).toBe(false);
  });
});

describe("camera style + layout regions", () => {
  it("setCamera patches the camera only", () => {
    const doc = base();
    const out = setCamera(doc, { preset: "dual-frame", mirror: true });
    expect(out.camera.preset).toBe("dual-frame");
    expect(out.camera.mirror).toBe(true);
    expect(out.camera.anchor).toBe(doc.camera.anchor);
    expect(out.clips).toBe(doc.clips);
    expect(doc.camera.preset).toBe("no-webcam");
  });

  it("the standard roundness follows the mask", () => {
    expect(cameraRoundness({ roundness: null, mask: "square" })).toBe(0.7);
    expect(cameraRoundness({ roundness: null, mask: "rectangle" })).toBe(0.4);
    expect(cameraRoundness({ roundness: 0.1, mask: "square" })).toBe(0.1);
  });

  it("adds a layout region clamped into the clip, replacing one on the same start", () => {
    const doc = base({ clips: [{ startMs: 1000, endMs: 5000 }], crop: [null] });
    const a = addLayoutRegion(doc, 0, 500, "dual-frame");
    expect(a.layoutRegions.map((r) => [r.clip, r.startMs, r.preset])).toEqual([[0, 1000, "dual-frame"]]);
    const b = addLayoutRegion(a, 0, 1000, "camera-only");
    expect(b.layoutRegions.map((r) => [r.startMs, r.preset])).toEqual([[1000, "camera-only"]]);
    const c = addLayoutRegion(b, 0, 3000.4, "vertical-stack");
    expect(c.layoutRegions.map((r) => r.startMs)).toEqual([1000, 3000]);
    expect(new Set(c.layoutRegions.map((r) => r.id)).size).toBe(2);
    expect(addLayoutRegion(c, 7, 1000, "dual-frame")).toBe(c);
    // At or after the clip's end: nothing to change, the same doc (no undo step).
    expect(addLayoutRegion(c, 0, 5000, "dual-frame")).toBe(c);
    expect(addLayoutRegion(c, 0, 9000, "dual-frame")).toBe(c);
    expect(addLayoutRegion(c, 0, 4999.6, "dual-frame")).toBe(c);
    expect(removeLayoutRegion(c, c.layoutRegions[0].id).layoutRegions.map((r) => r.startMs)).toEqual([3000]);
  });

  it("a split carries the layout in force at the cut into the second half", () => {
    const doc = base({
      clips: [{ startMs: 0, endMs: 10_000 }, { startMs: 0, endMs: 2_000 }],
      crop: [null, null],
      layoutRegions: [
        { id: "a", clip: 0, startMs: 1000, preset: "dual-frame" },
        { id: "b", clip: 0, startMs: 6000, preset: "camera-only" },
        { id: "c", clip: 1, startMs: 500, preset: "vertical-stack" },
      ],
    });
    const out = splitAt(doc, 0, 4000);
    const regs = out.layoutRegions.map((r) => [r.clip, r.startMs, r.preset]);
    expect(regs).toEqual([
      [0, 1000, "dual-frame"],
      [1, 6000, "camera-only"],
      [2, 500, "vertical-stack"],
      [1, 4000, "dual-frame"],
    ]);
    // Before any region: nothing to carry (the global preset holds on both halves).
    const plain = splitAt(base({ layoutRegions: [{ id: "x", clip: 0, startMs: 6000, preset: "dual-frame" }] }), 0, 4000);
    expect(plain.layoutRegions.map((r) => [r.clip, r.startMs])).toEqual([[1, 6000]]);
    // A region exactly at the cut moves along; no copy is added.
    const atCut = splitAt(base({ layoutRegions: [{ id: "y", clip: 0, startMs: 4000, preset: "dual-frame" }] }), 0, 4000);
    expect(atCut.layoutRegions.map((r) => [r.id, r.clip, r.startMs])).toEqual([["y", 1, 4000]]);
    // Deleting a clip takes its regions along, later ones move down.
    const del = deleteClip(out, 0);
    expect(del.layoutRegions.map((r) => [r.clip, r.startMs])).toEqual([[0, 6000], [1, 500], [0, 4000]]);
  });

  it("the camera constants mirror record/editor.rs", () => {
    const num = (name: string) => {
      const m = new RegExp(`pub const ${name}: [^=]+= ([^;]+);`).exec(EDITOR_RS);
      expect(m, name).not.toBeNull();
      return Number(m![1]);
    };
    expect(num("DEFAULT_CAMERA_SIZE")).toBe(DEFAULT_CAMERA_SIZE);
    expect(num("CAMERA_ROUNDNESS_SQUARE")).toBe(CAMERA_ROUNDNESS_SQUARE);
    expect(num("CAMERA_ROUNDNESS_RECTANGLE")).toBe(CAMERA_ROUNDNESS_RECTANGLE);
    expect(num("DEFAULT_CAMERA_BLUR")).toBe(DEFAULT_CAMERA_BLUR);
    expect(num("LAYOUT_TRANSITION_SEC")).toBe(LAYOUT_TRANSITION_SEC);
    // The size range reads the layout module's limits.
    expect(EDITOR_RS).toMatch(/pub const CAMERA_SIZE_RANGE: \(f64, f64\) = \(super::layout::CAMERA_SIZE_MIN, super::layout::CAMERA_SIZE_MAX\);/);
    const LAYOUT_RS = import.meta.glob("../../../rust-lib/src/record/layout.rs", { query: "?raw", import: "default", eager: true });
    const layout = Object.values(LAYOUT_RS)[0] as string;
    expect(layout).toBeTruthy();
    const lim = (n: string) => Number(new RegExp(`pub const ${n}: f64 = ([\\d.]+);`).exec(layout)![1]);
    expect([lim("CAMERA_SIZE_MIN"), lim("CAMERA_SIZE_MAX")]).toEqual([...CAMERA_SIZE_RANGE]);
  });
});

describe("captions (stage 6)", () => {
  it("the caption constants mirror record/captions.rs", () => {
    const raw = (name: string) => {
      const m = new RegExp(`pub const ${name}: [^=]+= ([^;]+);`).exec(CAPTIONS_RS);
      expect(m, name).not.toBeNull();
      return m![1];
    };
    const num = (name: string) => Number(raw(name));
    const tuple = (name: string) => raw(name).replace(/[()]/g, "").split(",").map((v) => Number(v.trim()));
    expect(tuple("CAPTION_FONT_SIZE_RANGE")).toEqual([...CAPTION_FONT_SIZE_RANGE]);
    expect(tuple("CAPTION_FONT_SIZE_UI_RANGE")).toEqual([...CAPTION_FONT_SIZE_UI_RANGE]);
    expect(tuple("CAPTION_PLATE_OPACITY_RANGE")).toEqual([...CAPTION_PLATE_OPACITY_RANGE]);
    expect(tuple("CAPTION_WORDS_RANGE")).toEqual([...CAPTION_WORDS_RANGE]);
    expect(num("CAPTION_PLATE_OFF_BELOW")).toBe(CAPTION_PLATE_OFF_BELOW);
    expect(num("CAPTION_INSET_Y_MAX")).toBe(CAPTION_INSET_Y_MAX);
    expect(num("CAPTION_INSET_X_MAX")).toBe(CAPTION_INSET_X_MAX);
    expect(num("CAPTION_INSET_STEP")).toBe(CAPTION_INSET_STEP);
    expect(num("CAPTION_LANDSCAPE_ASPECT")).toBe(CAPTION_LANDSCAPE_ASPECT);
    expect(num("CAPTION_INSET_Y_LANDSCAPE")).toBe(CAPTION_INSET_Y_LANDSCAPE);
    expect(num("CAPTION_INSET_Y_PORTRAIT")).toBe(CAPTION_INSET_Y_PORTRAIT);
    expect(num("CAPTION_INSET_X_LANDSCAPE")).toBe(CAPTION_INSET_X_LANDSCAPE);
    expect(num("CAPTION_INSET_X_PORTRAIT")).toBe(CAPTION_INSET_X_PORTRAIT);
    expect(num("DEFAULT_CAPTION_MIN_WORDS")).toBe(CAPTION_DEFAULTS.minWords);
    expect(num("DEFAULT_CAPTION_MAX_WORDS")).toBe(CAPTION_DEFAULTS.maxWords);
    expect(JSON.parse(raw("CAPTION_PLATE_DARK"))).toBe(CAPTION_PLATE_DARK);
    expect(JSON.parse(raw("CAPTION_PLATE_LIGHT"))).toBe(CAPTION_PLATE_LIGHT);
    // The looks, entry by entry.
    const block = /pub const CAPTION_LOOKS: [^=]+= \[([\s\S]*?)\];/.exec(CAPTIONS_RS);
    expect(block).not.toBeNull();
    const rust = [...block![1].matchAll(/\("(\w+)", CaptionLook \{ ([^}]*) \}\)/g)].map((m) => {
      const f = Object.fromEntries(m[2].split(", ").map((kv) => kv.split(": ")));
      return [
        m[1],
        {
          fontFamily: JSON.parse(f.font_family),
          bold: f.bold === "true",
          fontSize: Number(f.font_size),
          color: JSON.parse(f.color),
          plate: f.plate === "true",
          plateColor: JSON.parse(f.plate_color),
          plateOpacity: Number(f.plate_opacity),
        },
      ];
    });
    expect(rust).toHaveLength(4);
    expect(rust).toEqual(CAPTION_LOOKS.map(([n, l]) => [n, { ...l }]));
  });

  it("the defaults are the classic look, off, with the aspect's insets", () => {
    expect(CAPTION_DEFAULTS.enabled).toBe(false);
    expect(CAPTION_DEFAULTS.fontSize).toBe(48);
    expect([CAPTION_DEFAULTS.insetY, CAPTION_DEFAULTS.insetX]).toEqual([null, null]);
    expect(captionInsets(CAPTION_DEFAULTS, 16 / 9)).toEqual({ y: 1.5, x: 10 });
    expect(captionInsets(CAPTION_DEFAULTS, 9 / 16)).toEqual({ y: 12.5, x: 5 });
    expect(captionInsets({ ...CAPTION_DEFAULTS, insetY: 70, insetX: 3 }, 9 / 16)).toEqual({ y: 50, x: 3 });
  });
});


describe("transcript cut (stage 6)", () => {
  it("split_puts_a_spanning_zoom_into_both_halves", () => {
    const doc = addZoom(base(), 0, 1000); // 1000–3000
    const out = splitAt(doc, 0, 2000);
    expect(out.zooms.map((z) => [z.clip, z.startMs, z.endMs])).toEqual([[0, 1000, 2000], [1, 2000, 3000]]);
    expect(out.zooms[0].id).toBe(doc.zooms[0].id);
    expect(out.zooms[1].id).not.toBe(doc.zooms[0].id);
    const { id: _a, clip: _b, startMs: _c, endMs: _d, ...rest0 } = out.zooms[0];
    const { id: _e, clip: _f, startMs: _g, endMs: _h, ...rest1 } = out.zooms[1];
    expect(rest1).toEqual(rest0);
  });

  it("split_puts_a_spanning_annotation_into_both_halves", () => {
    const doc = addAnnotation(base(), "text", 0, 1000); // 1000–3000
    const out = splitAt(doc, 0, 2500);
    expect(out.annotations.map((a) => [a.clip, a.startMs, a.endMs])).toEqual([[0, 1000, 2500], [1, 2500, 3000]]);
    expect(new Set(out.annotations.map((a) => a.id)).size).toBe(2);
    expect(out.annotations[1].body).toEqual(out.annotations[0].body);
  });

  it("cut_source_range_removes_the_middle", () => {
    const doc = base({ speed: [{ clip: 0, startMs: 1000, endMs: 9000, speed: 2 }] });
    const out = cutSourceRange(doc, 0, 4000, 6000);
    expect(out.clips).toEqual([
      { startMs: 0, endMs: 4000 },
      { startMs: 6000, endMs: 10_000 },
    ]);
    expect(out.crop).toEqual([null, null]);
    expect(out.speed).toEqual([
      { clip: 0, startMs: 1000, endMs: 4000, speed: 2 },
      { clip: 1, startMs: 6000, endMs: 9000, speed: 2 },
    ]);
  });

  it("cut_near_an_edge_moves_the_edge", () => {
    expect(cutSourceRange(base(), 0, 50, 3000).clips).toEqual([{ startMs: 3000, endMs: 10_000 }]);
    expect(cutSourceRange(base(), 0, 7000, 9_950).clips).toEqual([{ startMs: 0, endMs: 7000 }]);
    // Exactly MIN_CLIP_MS away is still a real split.
    expect(cutSourceRange(base(), 0, 100, 3000).clips.length).toBe(2);
  });

  it("cut_of_the_whole_clip_deletes_it_but_never_the_last", () => {
    const one = base();
    expect(cutSourceRange(one, 0, 20, 9_990)).toBe(one);
    const two = base({ clips: [{ startMs: 0, endMs: 4000 }, { startMs: 5000, endMs: 9000 }], crop: [null, null] });
    expect(cutSourceRange(two, 1, 5000, 9000).clips).toEqual([{ startMs: 0, endMs: 4000 }]);
  });

  it("restore_gap_merges_neighbours_and_rejoins_split_regions", () => {
    let doc = addZoom(base(), 0, 3000); // 3000–5000
    doc = addAnnotation(doc, "arrow", 0, 3500); // 3500–5500
    doc = { ...doc, speed: [{ clip: 0, startMs: 2000, endMs: 8000, speed: 2 }] };
    const cut = cutSourceRange(doc, 0, 4000, 4500);
    expect(cut.clips.length).toBe(2);
    expect(cut.zooms.length).toBe(2);
    expect(canRestoreGap(cut, 0)).toBe(true);
    const back = restoreGap(cut, 0);
    expect(back.clips).toEqual([{ startMs: 0, endMs: 10_000 }]);
    expect(back.crop).toEqual([null]);
    expect(back.zooms.map((z) => [z.id, z.clip, z.startMs, z.endMs])).toEqual([[doc.zooms[0].id, 0, 3000, 5000]]);
    expect(back.annotations.map((a) => [a.id, a.clip, a.startMs, a.endMs])).toEqual([
      [doc.annotations[0].id, 0, 3500, 5500],
    ]);
    expect(back.speed).toEqual([{ clip: 0, startMs: 2000, endMs: 8000, speed: 2 }]);
    // A changed half is not rejoined (it is no longer the same region).
    const edited = { ...cut, zooms: cut.zooms.map((z, i) => (i === 1 ? { ...z, depth: 6 } : z)) };
    expect(restoreGap(edited, 0).zooms.length).toBe(2);
  });

  it("cut_shorter_than_min_clip_keeps_the_tail", () => {
    const doc = addZoom(base(), 0, 3000); // 3000–5000 spans the cut
    const out = cutSourceRange(doc, 0, 4000, 4050);
    expect(out.clips).toEqual([
      { startMs: 0, endMs: 4000 },
      { startMs: 4050, endMs: 10_000 },
    ]);
    expect(out.zooms.map((z) => [z.clip, z.startMs, z.endMs])).toEqual([[0, 3000, 4000], [1, 4050, 5000]]);
    // Any short cut anywhere keeps the full remaining tail (the old double split dropped it).
    for (const s of [500, 2000, 9000]) {
      const r = cutSourceRange(base(), 0, s, s + 1);
      expect(r.clips).toEqual([{ startMs: 0, endMs: s }, { startMs: s + 1, endMs: 10_000 }]);
    }
    // A region wholly inside the cut is dropped; a later one is untouched.
    let d = addAnnotation(base(), "text", 0, 4100);
    d = { ...d, annotations: [{ ...d.annotations[0], startMs: 4100, endMs: 4400 }] };
    expect(cutSourceRange(d, 0, 4000, 4500).annotations).toEqual([]);
    const back = restoreGap(out, 0);
    expect(back.zooms.map((z) => [z.clip, z.startMs, z.endMs])).toEqual([[0, 3000, 5000]]);
  });

  it("restore_gap_pairs_halves_in_any_array_order", () => {
    let doc = addZoom(base(), 0, 3000);
    doc = { ...doc, speed: [{ clip: 0, startMs: 2000, endMs: 8000, speed: 2 }] };
    const cut = cutSourceRange(doc, 0, 4000, 4500);
    const reversed = { ...cut, zooms: [...cut.zooms].reverse(), speed: [...cut.speed].reverse() };
    expect(reversed.zooms[0].clip).toBe(1);
    const back = restoreGap(reversed, 0);
    expect(back.zooms.map((z) => [z.id, z.clip, z.startMs, z.endMs])).toEqual([[doc.zooms[0].id, 0, 3000, 5000]]);
    expect(back.speed).toEqual([{ clip: 0, startMs: 2000, endMs: 8000, speed: 2 }]);
  });

  it("restore_gap_refuses_different_crops", () => {
    const cut = cutSourceRange(base(), 0, 4000, 4500);
    expect(canRestoreGap(cut, 0)).toBe(true);
    const cropped = setCrop(cut, 1, { x: 0.1, y: 0.1, w: 0.5, h: 0.5 });
    expect(canRestoreGap(cropped, 0)).toBe(false);
    expect(restoreGap(cropped, 0)).toBe(cropped);
  });

  it("restore_gap_refuses_different_takes_or_reordered_clips", () => {
    const takes = base({
      clips: [{ startMs: 0, endMs: 4000 }, { startMs: 5000, endMs: 9000, source: "take-2" }],
      crop: [null, null],
    });
    expect(canRestoreGap(takes, 0)).toBe(false);
    expect(restoreGap(takes, 0)).toBe(takes);
    const reordered = base({ clips: [{ startMs: 5000, endMs: 9000 }, { startMs: 0, endMs: 4000 }], crop: [null, null] });
    expect(canRestoreGap(reordered, 0)).toBe(false);
    expect(restoreGap(reordered, 0)).toBe(reordered);
    expect(canRestoreGap(reordered, 1)).toBe(false);
  });
});

describe("stage 7 fields", () => {
  it("a new zoom is flat (no rotation) and the base doc carries depth of field on, 3D cursor off", () => {
    const d = addZoom(base(), 0, 100);
    expect(d.zooms[0].rotation ?? null).toBeNull();
    expect(base().appearance.depthOfField).toBe(true);
    expect(base().cursor.model3d).toBe(false);
  });
});

describe("stage 7: rotation and effects", () => {
  const fx = (over = {}) => ({ id: "e1", clip: 0, startMs: 2000, endMs: 6000, effect: "vignette", values: {}, ...over });
  const withFx = (e = fx()) => base({ effects: [e] });

  it("setZoomRotation changes only rotation", () => {
    const z = addZoom(base(), 0, 1000);
    const id = z.zooms[0].id;
    const r = setZoomRotation(z, id, "orbit");
    expect(r.zooms[0]).toEqual({ ...z.zooms[0], rotation: "orbit" });
    expect(setZoomRotation(r, id, null).zooms[0]).toEqual(z.zooms[0]);
    expect(setZoomRotation(z, "nope", "left")).toBe(z);
  });

  it("add / set / move / delete", () => {
    const d = addEffect(base(), 0, 1000, 99_999, "grain");
    expect(d.effects).toHaveLength(1);
    expect(d.effects![0]).toMatchObject({ clip: 0, startMs: 1000, endMs: 10_000, effect: "grain", values: {} });
    expect(addEffect(base(), 0, 1000, 2000, "nope")).toEqual(base());
    const id = d.effects![0].id;
    const s = setEffectValue(d, id, "amount", 7);
    expect(s.effects![0].values).toEqual({ amount: 1 });
    expect(setEffectValue(s, id, "amount", 0.15).effects![0].values).toEqual({});
    expect(setEffectValue(d, id, "bogus", 1)).toBe(d);
    const m = moveEffect(d, id, 0, 3000, 4000);
    expect(m.effects![0]).toMatchObject({ startMs: 3000, endMs: 4000 });
    expect(deleteEffect(m, id).effects).toEqual([]);
  });

  it("splitAt puts a spanning effect in both halves, second with a fresh id", () => {
    const d = base({ effects: [fx()] });
    const out = splitAt(d, 0, 4000);
    expect(out.effects).toHaveLength(2);
    expect(out.effects![0]).toMatchObject({ id: "e1", clip: 0, startMs: 2000, endMs: 4000 });
    expect(out.effects![1]).toMatchObject({ clip: 1, startMs: 4000, endMs: 6000, effect: "vignette" });
    expect(out.effects![1].id).not.toBe("e1");
  });

  it("restoreGap rejoins the halves", () => {
    const split = splitAt(withFx(), 0, 4000);
    const joined = restoreGap(split, 0);
    expect(joined.effects).toHaveLength(1);
    expect(joined.effects![0]).toMatchObject({ clip: 0, startMs: 2000, endMs: 6000 });
  });

  it("cutSourceRange keeps an effect outside and shortens one crossing the cut", () => {
    const outside = cutSourceRange(base({ effects: [fx({ startMs: 8000, endMs: 9500 })] }), 0, 2000, 4000);
    expect(outside.effects).toHaveLength(1);
    expect(outside.effects![0]).toMatchObject({ clip: 1, startMs: 8000, endMs: 9500 });
    const crossing = cutSourceRange(withFx(fx({ startMs: 3000, endMs: 7000 })), 0, 2000, 5000);
    expect(crossing.effects).toHaveLength(1);
    expect(crossing.effects![0]).toMatchObject({ clip: 1, startMs: 5000, endMs: 7000 });
    const head = cutSourceRange(withFx(fx({ startMs: 1000, endMs: 3000 })), 0, 2000, 5000);
    expect(head.effects![0]).toMatchObject({ clip: 0, startMs: 1000, endMs: 2000 });
  });

  it("deleteClip takes its effects along and re-indexes the rest", () => {
    const two = splitAt(base({ effects: [fx({ startMs: 1000, endMs: 2000 }), fx({ id: "e2", startMs: 6000, endMs: 7000 })] }), 0, 4000);
    const out = deleteClip(two, 0);
    expect(out.effects!.map((e) => [e.id, e.clip])).toEqual([["e2", 0]]);
  });
});
