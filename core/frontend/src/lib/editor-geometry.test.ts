import { describe, it, expect } from "vitest";
import {
  dragRect,
  makeDragAnnotation,
  nextStepNumber,
  type Annotation,
} from "./editor-geometry";

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

  it("arrow and line keep raw endpoints", () => {
    expect(makeDragAnnotation("arrow", a, b, "#fff", 4)).toEqual({
      type: "arrow",
      x1: 0,
      y1: 0,
      x2: 30,
      y2: 40,
      color: "#fff",
      width: 4,
    });
    expect(makeDragAnnotation("line", a, b, "#fff", 2)).toMatchObject({ type: "line", x2: 30 });
  });

  it("rect and ellipse use a normalised rect", () => {
    expect(makeDragAnnotation("rect", b, a, "#f00", 3)).toMatchObject({
      type: "rect",
      x: 0,
      y: 0,
      w: 30,
      h: 40,
    });
    expect(makeDragAnnotation("ellipse", a, b, "#f00", 3)).toMatchObject({
      type: "ellipse",
      w: 30,
      h: 40,
    });
  });

  it("highlight is forced yellow", () => {
    expect(makeDragAnnotation("highlight", a, b, "#123456", 4)).toMatchObject({
      type: "highlight",
      color: "#facc15",
    });
  });

  it("blur block size scales with width, min 6", () => {
    expect(makeDragAnnotation("blur", a, b, "#fff", 1)).toMatchObject({ blockSize: 6 });
    expect(makeDragAnnotation("blur", a, b, "#fff", 10)).toMatchObject({ blockSize: 30 });
  });

  it("redact has no colour field (always opaque black at draw time)", () => {
    const r = makeDragAnnotation("redact", a, b, "#fff", 4);
    expect(r).toMatchObject({ type: "redact", x: 0, y: 0, w: 30, h: 40 });
  });

  it("click-placed tools (text, step) return null from the drag path", () => {
    expect(makeDragAnnotation("text", a, b, "#fff", 4)).toBeNull();
    expect(makeDragAnnotation("step", a, b, "#fff", 4)).toBeNull();
  });
});

describe("nextStepNumber", () => {
  it("starts at 1 when there are no step badges", () => {
    expect(nextStepNumber([])).toBe(1);
    const noSteps: Annotation[] = [
      { type: "rect", x: 0, y: 0, w: 1, h: 1, color: "#fff", width: 1 },
    ];
    expect(nextStepNumber(noSteps)).toBe(1);
  });
  it("is one past the highest existing badge (robust to gaps / undo)", () => {
    const anns: Annotation[] = [
      { type: "step", x: 0, y: 0, number: 1, color: "#f00", size: 16 },
      { type: "step", x: 0, y: 0, number: 3, color: "#f00", size: 16 },
    ];
    expect(nextStepNumber(anns)).toBe(4);
  });
  it("order of annotations doesn't matter", () => {
    const anns: Annotation[] = [
      { type: "step", x: 0, y: 0, number: 5, color: "#f00", size: 16 },
      { type: "step", x: 0, y: 0, number: 2, color: "#f00", size: 16 },
    ];
    expect(nextStepNumber(anns)).toBe(6);
    expect(nextStepNumber([...anns].reverse())).toBe(6);
  });
  it("recovers from zero / negative badge numbers", () => {
    const anns: Annotation[] = [
      { type: "step", x: 0, y: 0, number: 0, color: "#f00", size: 16 },
      { type: "step", x: 0, y: 0, number: -3, color: "#f00", size: 16 },
    ];
    expect(nextStepNumber(anns)).toBe(1);
  });
});

describe("dragRect — negative-coordinate space", () => {
  it("normalises drags entirely in negative coordinates", () => {
    expect(dragRect({ x: -10, y: -5 }, { x: -2, y: -1 })).toEqual({
      x: -10,
      y: -5,
      w: 8,
      h: 4,
    });
  });
  it("handles a drag crossing the origin", () => {
    expect(dragRect({ x: 5, y: -3 }, { x: -5, y: 3 })).toEqual({ x: -5, y: -3, w: 10, h: 6 });
  });
});

describe("makeDragAnnotation — shape payload details", () => {
  const a = { x: 0, y: 0 };
  const b = { x: 30, y: 40 };

  it("an unknown tool yields null (no half-built annotation)", () => {
    expect(makeDragAnnotation("bogus" as never, a, b, "#fff", 4)).toBeNull();
  });

  it("blur block size boundary: width 2 stays at the 6px minimum, width 3 exceeds it", () => {
    expect(makeDragAnnotation("blur", a, b, "#fff", 2)).toMatchObject({ blockSize: 6 });
    expect(makeDragAnnotation("blur", a, b, "#fff", 3)).toMatchObject({ blockSize: 9 });
  });

  it("highlight and redact carry no stroke width (fill-only shapes)", () => {
    const h = makeDragAnnotation("highlight", a, b, "#fff", 4)!;
    const r = makeDragAnnotation("redact", a, b, "#fff", 4)!;
    expect("width" in h).toBe(false);
    expect("width" in r).toBe(false);
    expect("color" in r).toBe(false); // redact is always opaque black at draw time
  });

  it("a reversed drag produces the same rect shape for every rect tool", () => {
    for (const tool of ["rect", "ellipse", "highlight", "blur", "redact"] as const) {
      const fwd = makeDragAnnotation(tool, a, b, "#fff", 4);
      const rev = makeDragAnnotation(tool, b, a, "#fff", 4);
      expect(rev).toEqual(fwd);
    }
  });

  it("arrow/line preserve drag direction (reversed drag ≠ same annotation)", () => {
    const fwd = makeDragAnnotation("arrow", a, b, "#fff", 4);
    const rev = makeDragAnnotation("arrow", b, a, "#fff", 4);
    expect(rev).not.toEqual(fwd); // the arrowhead sits at the release point
    expect(rev).toMatchObject({ x1: 30, y1: 40, x2: 0, y2: 0 });
  });
});
