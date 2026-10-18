import { describe, it, expect } from "vitest";
import {
  dragRect,
  normRect,
  makeDragAnnotation,
  nextStepNumber,
  boundsOf,
  hitTest,
  handlesFor,
  moveAnnotation,
  resizeAnnotation,
  restyleAnnotation,
  reflowSteps,
  normalizeAnnotation,
  type Annotation,
  type Style,
} from "./editor-geometry";

const S = (over: Partial<Style> = {}): Style => ({
  color: "#fff",
  width: 4,
  opacity: 1,
  shadow: false,
  fontSize: 28,
  ...over,
});

describe("dragRect", () => {
  it("normalises to a top-left anchored rect with non-negative size", () => {
    expect(dragRect({ x: 10, y: 20 }, { x: 4, y: 50 })).toEqual({
      x: 4,
      y: 20,
      w: 6,
      h: 30,
    });
  });
  it("handles a zero-size drag", () => {
    expect(dragRect({ x: 5, y: 5 }, { x: 5, y: 5 })).toEqual({ x: 5, y: 5, w: 0, h: 0 });
  });
});

describe("makeDragAnnotation", () => {
  const a = { x: 0, y: 0 };
  const b = { x: 30, y: 40 };

  it("arrow and line keep raw endpoints + carry id and style", () => {
    expect(makeDragAnnotation("arrow", a, b, S(), "a1")).toEqual({
      id: "a1",
      type: "arrow",
      x1: 0,
      y1: 0,
      x2: 30,
      y2: 40,
      color: "#fff",
      width: 4,
      opacity: 1,
      shadow: false,
    });
    expect(makeDragAnnotation("line", a, b, S(), "l1")).toMatchObject({ type: "line", x2: 30 });
  });

  it("carries the live opacity + shadow onto the shape", () => {
    expect(makeDragAnnotation("rect", a, b, S({ opacity: 0.5, shadow: true }), "r1")).toMatchObject({
      opacity: 0.5,
      shadow: true,
    });
  });

  it("rect and ellipse use a normalised rect", () => {
    expect(makeDragAnnotation("rect", b, a, S({ color: "#f00", width: 3 }), "r1")).toMatchObject({
      type: "rect",
      x: 0,
      y: 0,
      w: 30,
      h: 40,
    });
    expect(makeDragAnnotation("ellipse", a, b, S({ color: "#f00", width: 3 }), "e1")).toMatchObject({
      type: "ellipse",
      w: 30,
      h: 40,
    });
  });

  it("highlight is forced yellow", () => {
    expect(makeDragAnnotation("highlight", a, b, S({ color: "#123456" }), "h1")).toMatchObject({
      type: "highlight",
      color: "#facc15",
    });
  });

  it("blur block size scales with width, min 6", () => {
    expect(makeDragAnnotation("blur", a, b, S({ width: 1 }), "b1")).toMatchObject({ blockSize: 6 });
    expect(makeDragAnnotation("blur", a, b, S({ width: 10 }), "b2")).toMatchObject({ blockSize: 30 });
  });

  it("redact has no colour field (always opaque black at draw time)", () => {
    const r = makeDragAnnotation("redact", a, b, S(), "x1");
    expect(r).toMatchObject({ type: "redact", x: 0, y: 0, w: 30, h: 40 });
  });

  it("spotlight is a drag rect", () => {
    expect(makeDragAnnotation("spotlight", a, b, S(), "s1")).toMatchObject({
      type: "spotlight",
      x: 0,
      y: 0,
      w: 30,
      h: 40,
    });
  });

  it("non-drag tools (select, text, step, pen, crop, emoji) return null", () => {
    for (const tool of ["select", "text", "step", "pen", "crop", "emoji"] as const) {
      expect(makeDragAnnotation(tool, a, b, S(), "n")).toBeNull();
    }
  });
});

describe("nextStepNumber", () => {
  it("starts at 1 when there are no step badges", () => {
    expect(nextStepNumber([])).toBe(1);
    const noSteps: Annotation[] = [
      { id: "r", type: "rect", x: 0, y: 0, w: 1, h: 1, color: "#fff", width: 1 },
    ];
    expect(nextStepNumber(noSteps)).toBe(1);
  });
  it("is one past the highest existing badge (robust to gaps / undo)", () => {
    const anns: Annotation[] = [
      { id: "1", type: "step", x: 0, y: 0, number: 1, color: "#f00", size: 16 },
      { id: "2", type: "step", x: 0, y: 0, number: 3, color: "#f00", size: 16 },
    ];
    expect(nextStepNumber(anns)).toBe(4);
  });
});

describe("boundsOf", () => {
  it("rect/ellipse return their own rect, normalising negatives", () => {
    expect(boundsOf({ id: "1", type: "rect", x: 10, y: 20, w: 30, h: 40, color: "#f", width: 2 })).toEqual({
      x: 10,
      y: 20,
      w: 30,
      h: 40,
    });
    expect(boundsOf({ id: "2", type: "rect", x: 10, y: 20, w: -30, h: 40, color: "#f", width: 2 })).toEqual({
      x: -20,
      y: 20,
      w: 30,
      h: 40,
    });
  });
  it("arrow/line bound their endpoints regardless of direction", () => {
    expect(boundsOf({ id: "1", type: "arrow", x1: 0, y1: 0, x2: 30, y2: 40, color: "#f", width: 2 })).toEqual({
      x: 0,
      y: 0,
      w: 30,
      h: 40,
    });
    expect(boundsOf({ id: "2", type: "line", x1: 30, y1: 40, x2: 0, y2: 0, color: "#f", width: 2 })).toEqual({
      x: 0,
      y: 0,
      w: 30,
      h: 40,
    });
  });
  it("step is a square around its centre", () => {
    expect(boundsOf({ id: "1", type: "step", x: 50, y: 50, number: 1, color: "#f", size: 16 })).toEqual({
      x: 34,
      y: 34,
      w: 32,
      h: 32,
    });
  });
  it("emoji is centred on its point", () => {
    expect(boundsOf({ id: "1", type: "emoji", x: 50, y: 50, char: "🎯", size: 40 })).toEqual({
      x: 30,
      y: 30,
      w: 40,
      h: 40,
    });
  });
  it("pen bounds the point path", () => {
    expect(
      boundsOf({ id: "1", type: "pen", points: [{ x: 0, y: 0 }, { x: 10, y: 20 }, { x: 5, y: 5 }], color: "#f", width: 2 }),
    ).toEqual({ x: 0, y: 0, w: 10, h: 20 });
  });
});

describe("hitTest", () => {
  const rect: Annotation = { id: "r", type: "rect", x: 10, y: 10, w: 20, h: 20, color: "#f", width: 2 };
  it("hits inside a box and misses outside", () => {
    expect(hitTest([rect], { x: 15, y: 15 })).toBe("r");
    expect(hitTest([rect], { x: 100, y: 100 })).toBeNull();
  });
  it("hits a thin line within tolerance, misses far away", () => {
    const line: Annotation = { id: "l", type: "line", x1: 0, y1: 0, x2: 100, y2: 0, color: "#f", width: 2 };
    expect(hitTest([line], { x: 50, y: 3 })).toBe("l");
    expect(hitTest([line], { x: 50, y: 40 })).toBeNull();
  });
  it("returns the top-most (last-drawn) annotation when they overlap", () => {
    const lower: Annotation = { id: "lo", type: "rect", x: 0, y: 0, w: 50, h: 50, color: "#f", width: 2 };
    const upper: Annotation = { id: "up", type: "rect", x: 10, y: 10, w: 50, h: 50, color: "#f", width: 2 };
    expect(hitTest([lower, upper], { x: 20, y: 20 })).toBe("up");
  });
});

describe("handlesFor", () => {
  it("box shapes expose eight compass handles", () => {
    const hs = handlesFor({ id: "r", type: "rect", x: 10, y: 20, w: 30, h: 40, color: "#f", width: 2 });
    expect(hs).toHaveLength(8);
    expect(hs.find((h) => h.pos === "nw")).toEqual({ pos: "nw", x: 10, y: 20 });
    expect(hs.find((h) => h.pos === "se")).toEqual({ pos: "se", x: 40, y: 60 });
  });
  it("arrow/line expose two endpoint handles", () => {
    const hs = handlesFor({ id: "a", type: "arrow", x1: 1, y1: 2, x2: 3, y2: 4, color: "#f", width: 2 });
    expect(hs).toEqual([
      { pos: "start", x: 1, y: 2 },
      { pos: "end", x: 3, y: 4 },
    ]);
  });
  it("text exposes a single bottom-right scale handle", () => {
    const hs = handlesFor({ id: "t", type: "text", x: 10, y: 30, text: "ab", color: "#f", size: 20 });
    expect(hs).toHaveLength(1);
    expect(hs[0].pos).toBe("se");
  });
});

describe("moveAnnotation", () => {
  it("translates a box", () => {
    expect(moveAnnotation({ id: "r", type: "rect", x: 10, y: 20, w: 5, h: 5, color: "#f", width: 2 }, 5, 10)).toMatchObject({
      x: 15,
      y: 30,
    });
  });
  it("translates both endpoints of an arrow", () => {
    expect(moveAnnotation({ id: "a", type: "arrow", x1: 0, y1: 0, x2: 10, y2: 10, color: "#f", width: 2 }, 3, 4)).toMatchObject({
      x1: 3,
      y1: 4,
      x2: 13,
      y2: 14,
    });
  });
  it("translates every point of a pen stroke", () => {
    const moved = moveAnnotation(
      { id: "p", type: "pen", points: [{ x: 0, y: 0 }, { x: 10, y: 10 }], color: "#f", width: 2 },
      2,
      3,
    );
    expect(moved).toMatchObject({ points: [{ x: 2, y: 3 }, { x: 12, y: 13 }] });
  });
});

describe("resizeAnnotation", () => {
  const rect: Annotation = { id: "r", type: "rect", x: 10, y: 10, w: 20, h: 20, color: "#f", width: 2 };
  it("se handle grows width + height", () => {
    expect(resizeAnnotation(rect, "se", 10, 20)).toMatchObject({ x: 10, y: 10, w: 30, h: 40 });
  });
  it("nw handle moves origin and shrinks", () => {
    expect(resizeAnnotation(rect, "nw", 5, 5)).toMatchObject({ x: 15, y: 15, w: 15, h: 15 });
  });
  it("arrow endpoints move independently", () => {
    const arrow: Annotation = { id: "a", type: "arrow", x1: 0, y1: 0, x2: 10, y2: 10, color: "#f", width: 2 };
    expect(resizeAnnotation(arrow, "end", 5, 5)).toMatchObject({ x1: 0, y1: 0, x2: 15, y2: 15 });
    expect(resizeAnnotation(arrow, "start", 5, 5)).toMatchObject({ x1: 5, y1: 5, x2: 10, y2: 10 });
  });
  it("text se handle scales the font size (clamped ≥ 8)", () => {
    const text: Annotation = { id: "t", type: "text", x: 0, y: 0, text: "hi", color: "#f", size: 20 };
    expect(resizeAnnotation(text, "se", 10, 10)).toMatchObject({ size: 30 });
    expect(resizeAnnotation(text, "se", -100, -100)).toMatchObject({ size: 8 });
  });
  it("pen scales its points around the opposite corner", () => {
    const pen: Annotation = { id: "p", type: "pen", points: [{ x: 0, y: 0 }, { x: 10, y: 10 }], color: "#f", width: 2 };
    expect(resizeAnnotation(pen, "se", 10, 10)).toMatchObject({
      points: [{ x: 0, y: 0 }, { x: 20, y: 20 }],
    });
  });
});

describe("restyleAnnotation", () => {
  it("changes colour/width/opacity/shadow on a vector shape", () => {
    const rect: Annotation = { id: "r", type: "rect", x: 0, y: 0, w: 5, h: 5, color: "#f00", width: 2 };
    expect(restyleAnnotation(rect, { color: "#000", width: 8, opacity: 0.5, shadow: true })).toMatchObject({
      color: "#000",
      width: 8,
      opacity: 0.5,
      shadow: true,
    });
  });
  it("leaves a highlight's forced colour alone", () => {
    const hl: Annotation = { id: "h", type: "highlight", x: 0, y: 0, w: 5, h: 5, color: "#facc15" };
    expect(restyleAnnotation(hl, { color: "#000" })).toMatchObject({ color: "#facc15" });
  });
  it("ignores fields a shape doesn't have (redact stays bare)", () => {
    const red: Annotation = { id: "x", type: "redact", x: 0, y: 0, w: 5, h: 5 };
    const out = restyleAnnotation(red, { color: "#000", width: 8, opacity: 0.2, shadow: true });
    expect(out).toEqual(red);
  });
  it("fontSize patches text/step/emoji size", () => {
    const text: Annotation = { id: "t", type: "text", x: 0, y: 0, text: "x", color: "#f", size: 20 };
    expect(restyleAnnotation(text, { fontSize: 40 })).toMatchObject({ size: 40 });
  });
});

describe("reflowSteps", () => {
  it("renumbers step badges 1..N in array order, leaving others untouched", () => {
    const anns: Annotation[] = [
      { id: "s1", type: "step", x: 0, y: 0, number: 5, color: "#f", size: 16 },
      { id: "r", type: "rect", x: 0, y: 0, w: 1, h: 1, color: "#f", width: 1 },
      { id: "s2", type: "step", x: 0, y: 0, number: 2, color: "#f", size: 16 },
    ];
    const out = reflowSteps(anns);
    expect(out.filter((a) => a.type === "step").map((a) => (a as { number: number }).number)).toEqual([1, 2]);
    expect(out[1]).toBe(anns[1]); // non-step kept by reference
  });
});

describe("normRect / normalizeAnnotation", () => {
  it("normRect flips negative dimensions", () => {
    expect(normRect({ x: 10, y: 10, w: -5, h: -5 })).toEqual({ x: 5, y: 5, w: 5, h: 5 });
  });
  it("normalizeAnnotation fixes a box with negative size", () => {
    expect(
      normalizeAnnotation({ id: "r", type: "rect", x: 10, y: 10, w: -5, h: -5, color: "#f", width: 2 }),
    ).toMatchObject({ x: 5, y: 5, w: 5, h: 5 });
  });
  it("normalizeAnnotation leaves an arrow unchanged", () => {
    const arrow: Annotation = { id: "a", type: "arrow", x1: 5, y1: 5, x2: 0, y2: 0, color: "#f", width: 2 };
    expect(normalizeAnnotation(arrow)).toEqual(arrow);
  });
});
