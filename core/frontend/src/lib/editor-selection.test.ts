import { describe, it, expect } from "vitest";
import {
  axisLock,
  duplicate,
  hitCandidates,
  marqueeIds,
  pasteClip,
  pickAt,
  pressSelection,
  removeIds,
  reorder,
  resizeBy,
  sidebarStyle,
} from "./editor-selection";
import { boundsOf, type Annotation } from "./editor-geometry";
import { DEFAULT_PREFS, rememberStyle } from "./editor-prefs";

const arrow = (id: string, x1: number, y1: number, x2: number, y2: number): Annotation => ({
  id, type: "arrow", x1, y1, x2, y2, color: "#f00", width: 6,
});
const rect = (id: string, x: number, y: number, w: number, h: number, fill?: "outline" | "filled" | "tint"): Annotation => ({
  id, type: "rect", x, y, w, h, color: "#00f", width: 4, ...(fill ? { fill } : {}),
});
const step = (id: string, n: number, x = 0): Annotation => ({ id, type: "step", x, y: 0, number: n, color: "#f00", size: 14 });
const ids = (anns: Annotation[]) => anns.map((a) => a.id);
let counter = 0;
const newId = () => `n${++counter}`;

describe("pickAt — the reported bug", () => {
  // Arrow first, then an outline rectangle drawn around it.
  const doc = [arrow("arrow", 100, 100, 200, 150), rect("box", 50, 50, 200, 150)];

  it("a click on the arrow inside the box picks the arrow", () => {
    expect(pickAt(doc, { x: 150, y: 125 }, null)).toBe("arrow");
  });

  it("a click on the box's edge picks the box", () => {
    expect(pickAt(doc, { x: 50, y: 120 }, null)).toBe("box");
  });

  it("a click in the empty middle of an outline box picks nothing", () => {
    expect(pickAt(doc, { x: 120, y: 180 }, null)).toBeNull();
  });
});

describe("hit rules by fill", () => {
  it("tint behaves like outline", () => {
    expect(pickAt([rect("t", 0, 0, 100, 100, "tint")], { x: 50, y: 50 }, null)).toBeNull();
    expect(pickAt([rect("t", 0, 0, 100, 100, "tint")], { x: 1, y: 50 }, null)).toBe("t");
  });

  it("a filled rect is hit anywhere inside", () => {
    expect(pickAt([rect("f", 0, 0, 100, 100, "filled")], { x: 50, y: 50 }, null)).toBe("f");
  });

  it("an outline ellipse is hit on its rim, not its middle", () => {
    const e: Annotation = { id: "e", type: "ellipse", x: 0, y: 0, w: 200, h: 100, color: "#0f0", width: 4 };
    expect(pickAt([e], { x: 100, y: 50 }, null)).toBeNull();
    expect(pickAt([e], { x: 100, y: 1 }, null)).toBe("e");
  });

  it("the smaller of two filled elements wins, even when it is older", () => {
    const doc = [rect("inner", 40, 40, 20, 20, "filled"), rect("outer", 0, 0, 100, 100, "filled")];
    expect(pickAt(doc, { x: 50, y: 50 }, null)).toBe("inner");
  });

  it("candidates are ranked best first", () => {
    const doc = [rect("inner", 40, 40, 20, 20, "filled"), rect("outer", 0, 0, 100, 100, "filled")];
    expect(hitCandidates(doc, { x: 50, y: 50 }).map((c) => c.id)).toEqual(["inner", "outer"]);
  });
});

describe("click again cycles", () => {
  const doc = [rect("a", 0, 0, 100, 100, "filled"), rect("b", 10, 10, 100, 100, "filled")];
  const p = { x: 50, y: 50 };

  it("walks through everything under the pointer and wraps", () => {
    const first = pickAt(doc, p, null)!;
    const second = pickAt(doc, { x: 51, y: 52 }, { id: first, at: p })!;
    expect(second).not.toBe(first);
    expect(pickAt(doc, p, { id: second, at: p })).toBe(first);
  });

  it("a click elsewhere starts over", () => {
    const first = pickAt(doc, p, null)!;
    expect(pickAt(doc, p, { id: first, at: { x: 90, y: 90 } })).toBe(first);
  });

  it("a deleted or no-longer-hit previous pick is ignored", () => {
    expect(pickAt(doc, p, { id: "gone", at: p })).toBe(pickAt(doc, p, null));
  });
});

describe("marquee, order, duplicate, paste, remove", () => {
  it("marquee picks everything it touches, in document order", () => {
    const doc = [rect("a", 0, 0, 10, 10), rect("b", 50, 50, 10, 10), rect("c", 200, 200, 10, 10)];
    expect(marqueeIds(doc, { x: 5, y: 5, w: 50, h: 50 })).toEqual(["a", "b"]);
  });

  it("reorder front/back/forward/backward keeps the moved elements' order", () => {
    const doc = ["a", "b", "c", "d"].map((i) => rect(i, 0, 0, 1, 1));
    expect(ids(reorder(doc, ["a", "b"], "front"))).toEqual(["c", "d", "a", "b"]);
    expect(ids(reorder(doc, ["c", "d"], "back"))).toEqual(["c", "d", "a", "b"]);
    expect(ids(reorder(doc, ["b"], "forward"))).toEqual(["a", "c", "b", "d"]);
    expect(ids(reorder(doc, ["c"], "backward"))).toEqual(["a", "c", "b", "d"]);
    expect(ids(reorder(doc, ["d"], "forward"))).toEqual(["a", "b", "c", "d"]);
  });

  it("duplicate puts offset copies on top and renumbers steps", () => {
    const doc = [step("s1", 1), step("s2", 2, 40)];
    const r = duplicate(doc, ["s1"], 16, newId);
    expect(r.annotations).toHaveLength(3);
    expect(r.ids).toHaveLength(1);
    const copy = r.annotations[2] as Extract<Annotation, { type: "step" }>;
    expect(copy.id).toBe(r.ids[0]);
    expect(copy.x).toBe(16);
    expect(r.annotations.map((a) => (a as { number: number }).number)).toEqual([1, 2, 3]);
  });

  it("paste offsets copies and gives them fresh ids", () => {
    const clip = [rect("a", 0, 0, 10, 10)];
    const r = pasteClip([rect("a", 0, 0, 10, 10)], clip, 16, newId);
    expect(r.annotations).toHaveLength(2);
    expect(r.ids[0]).not.toBe("a");
    expect(r.annotations[1]).toMatchObject({ x: 16, y: 16 });
  });

  it("removeIds renumbers the remaining steps", () => {
    const doc = [step("s1", 1), step("s2", 2), step("s3", 3)];
    const out = removeIds(doc, ["s1", "s2"]);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ id: "s3", number: 1 });
  });
});

describe("axisLock and resizeBy", () => {
  it("axisLock keeps the dominant axis only when on", () => {
    expect(axisLock(10, 3, true)).toEqual({ dx: 10, dy: 0 });
    expect(axisLock(-2, 7, true)).toEqual({ dx: 0, dy: 7 });
    expect(axisLock(10, 3, false)).toEqual({ dx: 10, dy: 3 });
  });

  it("resizeBy steps width and size within bounds", () => {
    expect(resizeBy(arrow("a", 0, 0, 1, 1), 1)).toMatchObject({ width: 7 });
    expect(resizeBy({ ...arrow("a", 0, 0, 1, 1), width: 24 } as Annotation, 1)).toMatchObject({ width: 24 });
    expect(resizeBy({ ...arrow("a", 0, 0, 1, 1), width: 2 } as Annotation, -1)).toMatchObject({ width: 2 });
    expect(resizeBy(step("s", 1), 1)).toMatchObject({ size: 16 });
    const blur: Annotation = { id: "b", type: "blur", x: 0, y: 0, w: 5, h: 5, blockSize: 8 };
    expect(resizeBy(blur, 1)).toEqual(blur);
  });
});

describe("pressSelection", () => {
  it("a plain press on an unselected element selects it and starts a move", () => {
    expect(pressSelection([], ["a", "b"], "a", false)).toEqual({ next: ["a"], move: true, cycle: false });
  });
  it("a press on the current selection keeps it and may cycle", () => {
    expect(pressSelection(["a", "c"], ["a"], "a", false)).toEqual({ next: ["a", "c"], move: true, cycle: true });
  });
  it("shift-press adds and lets you drag the selection", () => {
    expect(pressSelection(["c"], ["a"], "a", true)).toEqual({ next: ["c", "a"], move: true, cycle: false });
  });
  it("shift-press that removes an element never drags the rest", () => {
    expect(pressSelection(["a", "c"], ["a"], "a", true)).toEqual({ next: ["c"], move: false, cycle: false });
  });
});

describe("sidebarStyle", () => {
  const prefs = rememberStyle(rememberStyle(DEFAULT_PREFS, "arrow", { color: "#ff0000", width: 8 }), "rect", { color: "#0000ff", width: 12 });
  const blueRect: Annotation = { id: "r", type: "rect", x: 0, y: 0, w: 9, h: 9, color: "#123456", width: 12, fill: "filled" };

  it("with nothing selected the sidebar shows the tool's remembered style", () => {
    expect(sidebarStyle(prefs, "arrow", undefined)).toMatchObject({ color: "#ff0000", width: 8 });
  });
  it("with one element selected it shows that element's style", () => {
    expect(sidebarStyle(prefs, "arrow", blueRect)).toMatchObject({ color: "#123456", width: 12, fill: "filled" });
  });
  it("after the selection ends the tool's style comes back (no leak)", () => {
    sidebarStyle(prefs, "arrow", blueRect);
    expect(sidebarStyle(prefs, "arrow", undefined)).toMatchObject({ color: "#ff0000", width: 8 });
  });
  it("select and crop keep the arrow's style as a neutral fallback", () => {
    expect(sidebarStyle(prefs, "select", undefined)).toMatchObject({ color: "#ff0000" });
  });
});

describe("text bounds follow the text style", () => {
  it("a pill's bounds cover its padding and height", () => {
    const plain: Annotation = { id: "t", type: "text", x: 100, y: 100, text: "Hi", color: "#fff", size: 20 };
    const pill: Annotation = { ...plain, textStyle: "pill" } as Annotation;
    const bp = boundsOf(plain);
    const bq = boundsOf(pill);
    expect(bq.x).toBeLessThan(bp.x);
    expect(bq.h).toBeGreaterThan(bp.h);
    // A click on the pill's left padding hits it.
    expect(pickAt([pill], { x: 94, y: 100 }, null, 0)).toBe("t");
  });
});
