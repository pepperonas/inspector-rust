import { describe, expect, it } from "vitest";
import type { Annotation, EditorDoc } from "./editor-geometry";
import { boundsOf } from "./editor-geometry";
import {
  ASPECTS,
  forwardPoint,
  IDENTITY,
  aspectCrop,
  constrainAspect,
  mapPoint,
  orientLines,
  orientOf,
  orientedSize,
  sourcePoint,
  transformDoc,
} from "./editor-transform";
import type { OcrLine } from "./editor-ocr";

const W = 400;
const H = 200;

describe("mapPoint / orientedSize", () => {
  it("rotates clockwise into an H×W image", () => {
    expect(mapPoint({ x: 10, y: 20 }, W, H, "cw")).toEqual({ x: 180, y: 10 });
    expect(orientedSize(W, H, { rot: 90, flip: false })).toEqual({ w: 200, h: 400 });
  });
  it("rotates counter-clockwise", () => {
    expect(mapPoint({ x: 10, y: 20 }, W, H, "ccw")).toEqual({ x: 20, y: 390 });
  });
  it("flips", () => {
    expect(mapPoint({ x: 10, y: 20 }, W, H, "flipH")).toEqual({ x: 390, y: 20 });
    expect(mapPoint({ x: 10, y: 20 }, W, H, "flipV")).toEqual({ x: 10, y: 180 });
  });
});

describe("orientOf", () => {
  it("four clockwise turns return to identity", () => {
    let o = IDENTITY;
    for (let i = 0; i < 4; i++) o = orientOf(o, "cw");
    expect(o).toEqual(IDENTITY);
  });
  it("cw then ccw is identity, flip twice is identity", () => {
    expect(orientOf(orientOf(IDENTITY, "cw"), "ccw")).toEqual(IDENTITY);
    expect(orientOf(orientOf(IDENTITY, "flipH"), "flipH")).toEqual(IDENTITY);
    expect(orientOf(orientOf(IDENTITY, "flipV"), "flipV")).toEqual(IDENTITY);
  });
  it("flipH then flipV equals a half turn", () => {
    expect(orientOf(orientOf(IDENTITY, "flipH"), "flipV")).toEqual({ rot: 180, flip: false });
  });
  it("sourcePoint agrees with stepwise mapPoint for every op sequence", () => {
    const seqs: ("cw" | "ccw" | "flipH" | "flipV")[][] = [
      ["cw"], ["flipH"], ["cw", "flipH"], ["flipV", "cw"], ["cw", "cw", "flipH", "ccw"], ["flipH", "flipV", "cw"],
    ];
    const p = { x: 37, y: 61 };
    for (const seq of seqs) {
      let q = p;
      let w = W;
      let h = H;
      let o = IDENTITY;
      for (const op of seq) {
        q = mapPoint(q, w, h, op);
        if (op === "cw" || op === "ccw") [w, h] = [h, w];
        o = orientOf(o, op);
      }
      // sourcePoint inverts the whole orientation in one step.
      expect(sourcePoint(q, W, H, o)).toEqual(p);
      expect(forwardPoint(p, W, H, o)).toEqual(q);
    }
  });
});

describe("transformDoc", () => {
  const rect: Annotation = { id: "r", type: "rect", x: 10, y: 20, w: 50, h: 30, color: "#f00", width: 4 };
  const arrow: Annotation = { id: "a", type: "arrow", x1: 0, y1: 0, x2: 100, y2: 50, cx: 40, cy: 10, color: "#f00", width: 4, style: "curved" };
  const step: Annotation = { id: "s", type: "step", x: 30, y: 40, number: 1, color: "#f00", size: 14 };
  const text: Annotation = { id: "t", type: "text", x: 10, y: 20, text: "hallo", color: "#f00", size: 20 };
  const pen: Annotation = { id: "p", type: "pen", points: [{ x: 1, y: 2 }, { x: 3, y: 4 }], color: "#f00", width: 2 };
  const doc: EditorDoc = { annotations: [rect, arrow, step, text, pen], crop: { x: 0, y: 0, w: 100, h: 50 }, background: null };

  it("moves box shapes, endpoints, bend point, pen points and the crop", () => {
    const d = transformDoc(doc, W, H, "cw");
    expect(d.annotations[0]).toMatchObject({ x: 150, y: 10, w: 30, h: 50 });
    expect(d.annotations[1]).toMatchObject({ x1: 200, y1: 0, x2: 150, y2: 100, cx: 190, cy: 40 });
    expect(d.annotations[2]).toMatchObject({ x: 160, y: 30 });
    expect((d.annotations[4] as { points: unknown }).points).toEqual([{ x: 198, y: 1 }, { x: 196, y: 3 }]);
    expect(d.crop).toEqual({ x: 150, y: 0, w: 50, h: 100 });
    expect(d.orient).toEqual({ rot: 90, flip: false });
  });

  it("keeps text upright: its box centre follows the turn", () => {
    const before = boundsOf(text);
    const d = transformDoc(doc, W, H, "cw");
    const after = boundsOf(d.annotations[3]);
    const c = mapPoint({ x: before.x + before.w / 2, y: before.y + before.h / 2 }, W, H, "cw");
    expect(after.x + after.w / 2).toBeCloseTo(c.x);
    expect(after.y + after.h / 2).toBeCloseTo(c.y);
    expect(after.w).toBeCloseTo(before.w);
  });

  it("is reversible", () => {
    const d = transformDoc(transformDoc(doc, W, H, "cw"), H, W, "ccw");
    expect(d.annotations).toEqual(doc.annotations);
    expect(d.crop).toEqual(doc.crop);
    expect(d.orient).toEqual(IDENTITY);
  });

  it("keeps the background and a missing crop", () => {
    const bg = { kind: "solid" as const, color: "#fff", padding: 10, radius: 4, shadow: false };
    const d = transformDoc({ annotations: [], crop: null, background: bg }, W, H, "flipH");
    expect(d.crop).toBeNull();
    expect(d.background).toBe(bg);
  });
});

describe("orientLines", () => {
  const lines: OcrLine[] = [{ t: "ab", b: [0.1, 0.2, 0.3, 0.1], w: [{ s: 0, e: 2, b: [0.1, 0.2, 0.3, 0.1] }] }];
  it("leaves identity alone", () => {
    expect(orientLines(lines, IDENTITY)).toBe(lines);
  });
  it("rotates normalised boxes with the image", () => {
    const [l] = orientLines(lines, { rot: 90, flip: false });
    // unit square turned clockwise: (x,y,w,h) -> (1-y-h, x, h, w)
    expect(l.b[0]).toBeCloseTo(0.7);
    expect(l.b[1]).toBeCloseTo(0.1);
    expect(l.b[2]).toBeCloseTo(0.1);
    expect(l.b[3]).toBeCloseTo(0.3);
    expect(l.w[0].b).toEqual(l.b);
    expect(l.t).toBe("ab");
  });
});

describe("crop aspect", () => {
  it("offers free plus the common ratios", () => {
    expect(ASPECTS.map((a) => a.id)).toEqual(["free", "1:1", "4:3", "3:2", "16:9", "9:16"]);
  });
  it("constrains a drag to the ratio in the drag direction", () => {
    const r = constrainAspect({ x: 100, y: 100 }, { x: 60, y: 90 }, 16 / 9, W, H);
    expect(r.w / r.h).toBeCloseTo(16 / 9);
    expect(r.x + r.w).toBeCloseTo(100);
    expect(r.y + r.h).toBeCloseTo(100);
    expect(r.w).toBeCloseTo(40);
  });
  it("uses the dominant axis", () => {
    const r = constrainAspect({ x: 0, y: 0 }, { x: 10, y: 100 }, 1, W, H);
    expect(r).toEqual({ x: 0, y: 0, w: 100, h: 100 });
  });
  it("shrinks to stay inside the image, keeping the ratio", () => {
    const r = constrainAspect({ x: 350, y: 150 }, { x: 420, y: 230 }, 1, W, H);
    expect(r.x + r.w).toBeLessThanOrEqual(W);
    expect(r.y + r.h).toBeLessThanOrEqual(H);
    expect(r.w).toBeCloseTo(r.h);
    expect(r.w).toBeCloseTo(50);
  });
  it("without a crop picks the largest centred rect", () => {
    expect(aspectCrop(null, 1, W, H)).toEqual({ x: 100, y: 0, w: 200, h: 200 });
  });
  it("with a crop keeps its centre and area, inside the image", () => {
    const r = aspectCrop({ x: 100, y: 50, w: 100, h: 100 }, 4, W, H);
    expect(r.w / r.h).toBeCloseTo(4);
    expect(r.x + r.w / 2).toBeCloseTo(150);
    expect(r.y + r.h / 2).toBeCloseTo(100);
    expect(r.w * r.h).toBeCloseTo(10000);
  });
  it("with a crop near the edge slides back inside", () => {
    const r = aspectCrop({ x: 350, y: 0, w: 50, h: 50 }, 2, W, H);
    expect(r.x + r.w).toBeLessThanOrEqual(W + 1e-9);
    expect(r.x).toBeGreaterThanOrEqual(0);
    expect(r.w / r.h).toBeCloseTo(2);
  });
});
