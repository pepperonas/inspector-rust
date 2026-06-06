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
});
