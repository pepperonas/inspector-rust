import { describe, it, expect } from "vitest";
import { arrowCurve, arrowHead, arrowOutline, defaultBend, distToArrow } from "./editor-arrows";
import { boundsOf, handlesFor, hitTest, moveAnnotation, resizeAnnotation, type ArrowAnnotation } from "./editor-geometry";

const A = (over: Partial<ArrowAnnotation> = {}): ArrowAnnotation => ({
  id: "a", type: "arrow", x1: 0, y1: 0, x2: 100, y2: 0, color: "#f00", width: 4, ...over,
});
const close = (p: { x: number; y: number }, q: { x: number; y: number }, eps = 0.5) =>
  Math.hypot(p.x - q.x, p.y - q.y) <= eps;
// Apex of the quadratic (t = 0.5).
const apex = (a: ArrowAnnotation) => ({
  x: 0.25 * a.x1 + 0.5 * (a.cx ?? 0) + 0.25 * a.x2,
  y: 0.25 * a.y1 + 0.5 * (a.cy ?? 0) + 0.25 * a.y2,
});

describe("arrow outlines", () => {
  it("classic is exactly the pre-stage-1 polygon", () => {
    const pts = arrowOutline(A());
    const want = [[0, 2], [78, 4], [78, 12.1], [100, 0], [78, -12.1], [78, -4], [0, -2]];
    expect(pts).toHaveLength(7);
    pts.forEach((p, i) => {
      expect(p.x).toBeCloseTo(want[i][0], 6);
      expect(p.y).toBeCloseTo(want[i][1], 6);
    });
  });

  it("an arrow without a style draws as classic", () => {
    expect(arrowOutline(A())).toEqual(arrowOutline(A({ style: "classic" })));
  });

  it("bold is broader than classic in body and head", () => {
    const maxY = (pts: { y: number }[]) => Math.max(...pts.map((p) => Math.abs(p.y)));
    const body = (pts: { x: number; y: number }[]) => Math.abs(pts[1].y);
    const c = arrowOutline(A());
    const b = arrowOutline(A({ style: "bold" }));
    expect(maxY(b)).toBeGreaterThan(maxY(c));
    expect(body(b)).toBeGreaterThan(body(c));
  });
});

describe("curved arrows", () => {
  const bent = () => {
    const c = defaultBend(0, 0, 100, 0);
    return A({ style: "curved", cx: c.x, cy: c.y });
  };

  it("default bend sits off the straight line", () => {
    const c = defaultBend(0, 0, 100, 0);
    expect(c.x).toBeCloseTo(50, 6);
    expect(Math.abs(c.y)).toBeCloseTo(18, 6);
  });

  it("the curve starts at the tail and passes through the apex", () => {
    const a = bent();
    const pts = arrowCurve(a);
    expect(close(pts[0], { x: 0, y: 0 }, 1e-6)).toBe(true);
    expect(pts.some((p) => close(p, apex(a), 1.5))).toBe(true);
  });

  it("a curved shaft stops where the head starts, a marker runs to the tip", () => {
    const end = (a: ArrowAnnotation) => arrowCurve(a)[arrowCurve(a).length - 1];
    expect(close(end(bent()), { x: 100, y: 0 }, 1)).toBe(false);
    expect(close(end({ ...bent(), style: "marker" }), { x: 100, y: 0 }, 1e-6)).toBe(true);
  });

  it("the head follows the end tangent, not the chord", () => {
    const h = arrowHead(bent());
    expect(close(h.tip, { x: 100, y: 0 }, 1e-6)).toBe(true);
    const base = { x: (h.left.x + h.right.x) / 2, y: (h.left.y + h.right.y) / 2 };
    // Straight arrow: base on the x-axis. Bent: base off it.
    expect(Math.abs(base.y)).toBeGreaterThan(1);
    expect(h.open).toBe(false);
    expect(arrowHead({ ...bent(), style: "marker" }).open).toBe(true);
  });

  it("hit-testing follows the curve", () => {
    const a = bent();
    const onCurve = apex(a);
    expect(distToArrow(a, onCurve)).toBeLessThan(1);
    expect(hitTest([a], onCurve)).toBe("a");
    // The chord midpoint is 9 px away from the curve's apex — not a hit.
    expect(hitTest([a], { x: 50, y: 0 })).toBeNull();
  });

  it("bounds contain the curve", () => {
    const a = bent();
    const b = boundsOf(a);
    const p = apex(a);
    expect(p.y).toBeGreaterThanOrEqual(b.y - 1e-6);
    expect(p.y).toBeLessThanOrEqual(b.y + b.h + 1e-6);
  });

  it("bent arrows get a mid handle at the apex, straight ones don't", () => {
    const a = bent();
    const mid = handlesFor(a).find((h) => h.pos === "mid");
    expect(mid && close(mid, apex(a), 1e-6)).toBe(true);
    expect(handlesFor(A()).some((h) => h.pos === "mid")).toBe(false);
  });

  it("move carries the bend", () => {
    const a = bent();
    const m = moveAnnotation(a, 10, 20) as ArrowAnnotation;
    expect(m.cx).toBeCloseTo((a.cx ?? 0) + 10, 6);
    expect(m.cy).toBeCloseTo((a.cy ?? 0) + 20, 6);
  });

  it("dragging an endpoint keeps the bend's shape", () => {
    const a = bent();
    const r = resizeAnnotation(a, "end", 20, 0) as ArrowAnnotation;
    expect(r.x2).toBe(120);
    expect(r.cx).toBeCloseTo((a.cx ?? 0) + 10, 6);
  });

  it("dragging mid moves the apex to the pointer", () => {
    const a = bent();
    const before = apex(a);
    const r = resizeAnnotation(a, "mid", 5, -7) as ArrowAnnotation;
    expect(close(apex(r), { x: before.x + 5, y: before.y - 7 }, 0.5)).toBe(true);
  });
});
