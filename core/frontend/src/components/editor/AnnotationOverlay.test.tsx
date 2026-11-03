// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAnnotation, RecordAnnotationBody } from "../../lib/ipc";
import { defaultAnnotationBody } from "../../lib/editor";
import { AnnotationOverlay, type AnnotationPatch } from "./AnnotationOverlay";

/** The picture box measures 1000 × 500 px: 1 % of the frame = 10 px across, 5 px down. */
beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    left: 0, top: 0, width: 1000, height: 500, right: 1000, bottom: 500, x: 0, y: 0, toJSON: () => ({}),
  } as DOMRect);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const ann = (body: RecordAnnotationBody, box = { x: 50, y: 50, w: 30, h: 20 }): RecordAnnotation => ({
  id: "a1",
  clip: 0,
  startMs: 0,
  endMs: 2_000,
  ...box,
  zIndex: 1,
  body,
});

function mount(a: RecordAnnotation, over: { screenRect?: [number, number, number, number] | null; zoomActive?: boolean; visible?: boolean } = {}) {
  const onLive = vi.fn<(p: AnnotationPatch) => void>();
  const onEnd = vi.fn();
  const onCommit = vi.fn<(p: AnnotationPatch) => void>();
  render(
    <AnnotationOverlay
      annotation={a}
      screenRect={over.screenRect ?? null}
      zoomActive={over.zoomActive ?? false}
      visible={over.visible}
      onLive={onLive}
      onEnd={onEnd}
      onCommit={onCommit}
    />,
  );
  return { onLive, onEnd, onCommit };
}

const drag = (el: Element, from: [number, number], to: [number, number]) => {
  fireEvent.pointerDown(el, { clientX: from[0], clientY: from[1], pointerId: 1 });
  fireEvent.pointerMove(el, { clientX: to[0], clientY: to[1], pointerId: 1 });
  fireEvent.pointerUp(el, { clientX: to[0], clientY: to[1], pointerId: 1 });
};

describe("AnnotationOverlay", () => {
  it("dragging the box moves x/y in percent of the frame and clamps at the edge", () => {
    const { onLive, onEnd, onCommit } = mount(ann(defaultAnnotationBody("text")));
    const box = screen.getByTestId("ed-ann-box");
    // +100 px across = +10 %, +50 px down = +10 %.
    drag(box, [600, 300], [700, 350]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 60, y: 60, w: 30, h: 20 });
    expect(onEnd).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
    // Far past the bottom-right edge: the box stops with its far edges on 100 %.
    drag(box, [600, 300], [5_000, 5_000]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 70, y: 80, w: 30, h: 20 });
    // And past the top-left edge it stops at 0.
    drag(box, [600, 300], [-5_000, -5_000]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 0, y: 0, w: 30, h: 20 });
  });

  it("a corner handle keeps the opposite corner where it was", () => {
    const { onLive } = mount(ann(defaultAnnotationBody("arrow"), { x: 20, y: 20, w: 40, h: 40 }));
    // The top-left corner pulled 100 px left and 50 px up: bottom-right (60, 60) stays.
    drag(screen.getByTestId("ed-ann-handle-nw"), [200, 100], [100, 50]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 10, y: 10, w: 50, h: 50 });
    // The bottom-right corner pulled past the edge: the box ends at 100 % and keeps its origin.
    drag(screen.getByTestId("ed-ann-handle-se"), [600, 300], [9_000, 9_000]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 20, y: 20, w: 80, h: 80 });
    // Pulled across the opposite corner: it stops at the smallest box, origin unchanged.
    drag(screen.getByTestId("ed-ann-handle-se"), [600, 300], [-9_000, -9_000]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 20, y: 20, w: 1, h: 1 });
  });

  it("an edge handle changes one side only", () => {
    const { onLive } = mount(ann(defaultAnnotationBody("text"), { x: 20, y: 20, w: 40, h: 40 }));
    drag(screen.getByTestId("ed-ann-handle-e"), [600, 200], [700, 400]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 20, y: 20, w: 50, h: 40 });
  });

  it("a blur box is measured in the clip's screen box, not the frame", () => {
    const blur = defaultAnnotationBody("blur");
    // The screen covers the right half of the frame: 1 % of it = 5 px across, 5 px down.
    const { onLive } = mount(ann(blur, { x: 10, y: 10, w: 20, h: 20 }), { screenRect: [0.5, 0, 0.5, 1] });
    const box = screen.getByTestId("ed-ann-box");
    expect(parseFloat(box.style.left)).toBeCloseTo(55);
    expect(parseFloat(box.style.width)).toBeCloseTo(10);
    drag(box, [600, 100], [650, 150]);
    expect(onLive).toHaveBeenLastCalledWith({ x: 20, y: 20, w: 20, h: 20 });
  });

  it("under an active zoom a blur shows no handles, only a note why", () => {
    mount(ann(defaultAnnotationBody("blur")), { screenRect: [0, 0, 1, 1], zoomActive: true });
    expect(screen.queryByTestId("ed-ann-box")).toBeNull();
    expect(screen.queryByTestId("ed-ann-handle-se")).toBeNull();
    expect(screen.getByTestId("ed-ann-zoom-note").textContent).toMatch(/zoom/i);
  });

  it("outside its time span the frame shows dimmed, with no handles and a note", () => {
    const { onLive } = mount(ann(defaultAnnotationBody("text")), { visible: false });
    expect(screen.queryByTestId("ed-ann-box")).toBeNull();
    expect(screen.queryByTestId("ed-ann-handle-se")).toBeNull();
    const hidden = screen.getByTestId("ed-ann-box-hidden");
    expect(parseFloat(hidden.style.left)).toBeCloseTo(50);
    expect(hidden.textContent).toBe("Not shown at the playhead");
    fireEvent.pointerDown(hidden, { clientX: 600, clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(hidden, { clientX: 900, clientY: 400, pointerId: 1 });
    fireEvent.pointerUp(hidden, { clientX: 900, clientY: 400, pointerId: 1 });
    expect(onLive).not.toHaveBeenCalled();
  });

  it("an active zoom does not hide the handles of a text (it stays in the frame)", () => {
    mount(ann(defaultAnnotationBody("text")), { zoomActive: true });
    expect(screen.getByTestId("ed-ann-handle-se")).toBeTruthy();
  });

  describe("free-form blur", () => {
    const square: [number, number][] = [
      [0, 0],
      [100, 0],
      [100, 100],
      [0, 100],
    ];
    const freehand = (points: [number, number][]) =>
      ann({ ...(defaultAnnotationBody("blur") as Extract<RecordAnnotationBody, { kind: "blur" }>), shape: "freehand", points }, { x: 0, y: 0, w: 100, h: 100 });

    it("a click inside the box inserts a point into the nearest edge", () => {
      const { onCommit, onLive } = mount(freehand(square));
      const box = screen.getByTestId("ed-ann-box");
      // (500, 25) px = (50 %, 5 %) of the box: nearest the top edge.
      fireEvent.pointerDown(box, { clientX: 500, clientY: 25, pointerId: 1 });
      fireEvent.pointerUp(box, { clientX: 500, clientY: 25, pointerId: 1 });
      expect(onLive).not.toHaveBeenCalled();
      expect(onCommit).toHaveBeenCalledWith({ points: [[0, 0], [50, 5], [100, 0], [100, 100], [0, 100]] });
    });

    it("dragging a point moves it (clamped into the box)", () => {
      const { onLive, onEnd } = mount(freehand(square));
      const p = screen.getAllByTestId("ed-ann-point")[2];
      drag(p, [1_000, 500], [800, 9_000]);
      expect(onLive).toHaveBeenLastCalledWith({ points: [[0, 0], [100, 0], [80, 100], [0, 100]] });
      expect(onEnd).toHaveBeenCalledTimes(1);
    });

    it("Delete removes the selected point, but never below three", () => {
      const { onCommit } = mount(freehand(square));
      fireEvent.pointerDown(screen.getAllByTestId("ed-ann-point")[1], { clientX: 1_000, clientY: 0, pointerId: 1 });
      fireEvent.pointerUp(screen.getAllByTestId("ed-ann-point")[1], { clientX: 1_000, clientY: 0, pointerId: 1 });
      const ev = new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true });
      document.body.dispatchEvent(ev);
      expect(ev.defaultPrevented).toBe(true);
      expect(onCommit).toHaveBeenCalledWith({ points: [[0, 0], [100, 100], [0, 100]] });

      cleanup();
      const three = mount(freehand(square.slice(0, 3)));
      fireEvent.pointerDown(screen.getAllByTestId("ed-ann-point")[0], { clientX: 0, clientY: 0, pointerId: 1 });
      fireEvent.pointerUp(screen.getAllByTestId("ed-ann-point")[0], { clientX: 0, clientY: 0, pointerId: 1 });
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true }));
      expect(three.onCommit).not.toHaveBeenCalled();
    });

    it("a point's Delete wins over window handlers registered earlier (the timeline's annotation delete)", () => {
      const earlier = vi.fn((e: KeyboardEvent) => e.defaultPrevented);
      window.addEventListener("keydown", earlier);
      try {
        mount(freehand(square));
        const p = screen.getAllByTestId("ed-ann-point")[1];
        fireEvent.pointerDown(p, { clientX: 1_000, clientY: 0, pointerId: 1 });
        fireEvent.pointerUp(p, { clientX: 1_000, clientY: 0, pointerId: 1 });
        document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true }));
        // The earlier handler either never sees the key or sees it already taken.
        expect(earlier.mock.results.every((r) => r.value === true)).toBe(true);
      } finally {
        window.removeEventListener("keydown", earlier);
      }
    });

    it("Delete without a selected point is left to the editor", () => {
      mount(freehand(square));
      const ev = new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true });
      document.body.dispatchEvent(ev);
      expect(ev.defaultPrevented).toBe(false);
    });
  });
});
