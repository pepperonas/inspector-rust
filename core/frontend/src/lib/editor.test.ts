import { describe, expect, it, vi } from "vitest";
import EDITOR_RS from "../../../rust-lib/src/record/editor.rs?raw";
import type { RecordEditDoc, RecordEditorView } from "./ipc";
import {
  ANNOTATION_DEFAULT_RECT,
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
  addTrack,
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
  cropForPreset,
  deleteClip,
  fitPreview,
  formatTimecode,
  setCrop,
  setTrim,
  trimSourceAt,
  splitAt,
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
  },
  zooms: [],
  autoZoomDone: true,
  annotations: [],
  audioTracks: [],
  systemGainDb: 0,
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

  it("splitAt gives a zoom to the half holding its start and cuts it; deleteClip re-indexes", () => {
    let doc = addZoom(base(), 0, 1000); // 1000–3000
    doc = addZoom(doc, 0, 6000); // 6000–8000
    const out = splitAt(doc, 0, 2000);
    expect(out.zooms.map((z) => [z.clip, z.startMs, z.endMs])).toEqual([[0, 1000, 2000], [1, 6000, 8000]]);
    const del = deleteClip(out, 0);
    expect(del.zooms.map((z) => [z.clip, z.startMs])).toEqual([[0, 6000]]);
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
    expect(split.annotations.map((a) => [a.clip, a.startMs, a.endMs])).toEqual([[0, 1_000, 2_000], [1, 6_000, 8_000]]);
    expect(split.audioTracks.map((t) => t.clip)).toEqual([0, 1]);
    const del = deleteClip(split, 0);
    expect(del.annotations.map((a) => a.clip)).toEqual([0]);
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
