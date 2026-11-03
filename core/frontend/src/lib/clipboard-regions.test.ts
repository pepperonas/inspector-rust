import { describe, expect, it } from "vitest";
import type { RecordAnnotation, RecordAudioTrack, RecordEditDoc, RecordZoomRegion } from "./ipc";
import { defaultAnnotationBody } from "./editor";
import { copyRegions, cutRegions, pasteRegions, selectionAfterPaste } from "./clipboard-regions";

const ann = (id: string, clip: number, startMs: number, endMs: number, zIndex = 1): RecordAnnotation => ({
  id,
  clip,
  startMs,
  endMs,
  x: 20,
  y: 30,
  w: 10,
  h: 10,
  zIndex,
  body: { ...defaultAnnotationBody("blur"), points: [[0, 0], [100, 0], [50, 100]] } as RecordAnnotation["body"],
});
const zoom = (id: string, clip: number, startMs: number, endMs: number): RecordZoomRegion => ({
  id,
  clip,
  startMs,
  endMs,
  depth: 4,
  customScale: null,
  focusX: 0.3,
  focusY: 0.7,
  focusMode: "manual",
  origin: "auto",
  hideCursor: true,
});
const track = (id: string): RecordAudioTrack => ({
  id,
  kind: "music",
  file: "song.m4a",
  clip: 0,
  startMs: 500,
  durationMs: 30_000,
  offsetMs: 1_000,
  spanMs: null,
  gainDb: -18,
  looped: true,
  fadeInMs: 1_000,
  fadeOutMs: 1_000,
  muted: false,
  label: "Bed",
});
const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 3,
    clips: [
      { startMs: 0, endMs: 5_000 },
      { startMs: 6_000, endMs: 11_000 },
    ],
    speed: [],
    crop: [null, null],
    zooms: [],
    annotations: [],
    audioTracks: [],
    ...over,
  }) as RecordEditDoc;

describe("copy and paste of regions", () => {
  it("an annotation pastes as a copy with a new id at the playhead, on top", () => {
    const d = doc({ annotations: [ann("a1", 0, 1_000, 2_500, 3)] });
    const data = copyRegions(d, { kind: "annotation", id: "a1" });
    expect(data).not.toBeNull();
    const out = pasteRegions(d, 1, 7_000, data!);
    expect(out.annotations).toHaveLength(2);
    const made = out.annotations[1];
    expect(made.id).not.toBe("a1");
    expect(made).toMatchObject({ clip: 1, startMs: 7_000, endMs: 8_500, x: 20, y: 30, zIndex: 4 });
    // The copy owns its outline: editing it never moves the original's points.
    expect(made.body).toEqual(d.annotations[0].body);
    if (made.body.kind === "blur" && d.annotations[0].body.kind === "blur") expect(made.body.points).not.toBe(d.annotations[0].body.points);
    expect(selectionAfterPaste(data!, out)).toEqual({ kind: "annotation", id: made.id });
  });

  it("a paste past the clip end is cut at the clip end", () => {
    const d = doc({ annotations: [ann("a1", 0, 1_000, 3_000)] });
    const out = pasteRegions(d, 1, 10_000, copyRegions(d, { kind: "annotation", id: "a1" })!);
    expect(out.annotations[1]).toMatchObject({ clip: 1, startMs: 10_000, endMs: 11_000 });
  });

  it("a paste position outside the clip clamps into it", () => {
    const d = doc({ zooms: [zoom("z1", 0, 0, 1_000)] });
    const out = pasteRegions(d, 1, 2_000, copyRegions(d, { kind: "zoom", id: "z1" })!);
    expect(out.zooms[1]).toMatchObject({ clip: 1, startMs: 6_000, endMs: 7_000 });
  });

  it("a pasted zoom is the user's (manual) and keeps its look", () => {
    const d = doc({ zooms: [zoom("z1", 0, 1_000, 2_000)] });
    const out = pasteRegions(d, 0, 3_000, copyRegions(d, { kind: "zoom", id: "z1" })!);
    expect(out.zooms[1]).toMatchObject({ startMs: 3_000, endMs: 4_000, depth: 4, focusX: 0.3, focusMode: "manual", hideCursor: true, origin: "manual" });
    expect(out.zooms[1].id).not.toBe("z1");
  });

  it("a track pastes anchored at the playhead with its settings", () => {
    const d = doc({ audioTracks: [track("t1")] });
    const out = pasteRegions(d, 1, 8_000, copyRegions(d, { kind: "track", id: "t1" })!);
    expect(out.audioTracks[1]).toMatchObject({ clip: 1, startMs: 8_000, offsetMs: 1_000, gainDb: -18, looped: true, label: "Bed", file: "song.m4a" });
    expect(out.audioTracks[1].id).not.toBe("t1");
  });

  it("nothing to copy for a clip, nothing or a missing id", () => {
    const d = doc({ annotations: [ann("a1", 0, 0, 1_000)] });
    expect(copyRegions(d, { kind: "clip", index: 0 })).toBeNull();
    expect(copyRegions(d, null)).toBeNull();
    expect(copyRegions(d, { kind: "annotation", id: "gone" })).toBeNull();
  });

  it("pasting into a clip that does not exist changes nothing", () => {
    const d = doc({ annotations: [ann("a1", 0, 0, 1_000)] });
    expect(pasteRegions(d, 9, 0, copyRegions(d, { kind: "annotation", id: "a1" })!)).toBe(d);
  });

  it("cut = copy + remove in one document", () => {
    const d = doc({ annotations: [ann("a1", 0, 0, 1_000), ann("a2", 0, 0, 1_000)] });
    const cut = cutRegions(d, { kind: "annotation", id: "a1" });
    expect(cut).not.toBeNull();
    expect(cut!.doc.annotations.map((a) => a.id)).toEqual(["a2"]);
    expect(cut!.data).toEqual(copyRegions(d, { kind: "annotation", id: "a1" }));
    expect(cutRegions(d, { kind: "clip", index: 0 })).toBeNull();
  });
});
