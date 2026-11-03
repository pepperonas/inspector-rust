// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { CameraOverlay, type CameraPatch } from "./CameraOverlay";

/** The picture box measures 1920 × 1080 px, the output too: 1 px = 1 output px. */
beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    left: 0, top: 0, width: 1920, height: 1080, right: 1920, bottom: 1080, x: 0, y: 0, toJSON: () => ({}),
  } as DOMRect);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

// A 432 × 243 box around the centre.
const RECT: [number, number, number, number] = [744 / 1920, 418.5 / 1080, 432 / 1920, 243 / 1080];

function mount() {
  const onLive = vi.fn<(p: CameraPatch) => void>();
  const onEnd = vi.fn();
  render(<CameraOverlay rect={RECT} outSize={[1920, 1080]} sizePct={40} onLive={onLive} onEnd={onEnd} />);
  return { onLive, onEnd };
}

describe("camera overlay", () => {
  it("only the box takes the pointer", () => {
    mount();
    expect(screen.getByTestId("ed-camera-overlay").className).toContain("pointer-events-none");
    expect(screen.getByTestId("ed-camera-box").className).toContain("pointer-events-auto");
    expect(screen.getAllByRole("button")).toHaveLength(4);
  });

  it("dragging the body moves the centre live and ends once", () => {
    const { onLive, onEnd } = mount();
    const box = screen.getByTestId("ed-camera-box");
    fireEvent.pointerDown(box, { clientX: 960, clientY: 540, pointerId: 1 });
    fireEvent.pointerMove(box, { clientX: 1152, clientY: 486, pointerId: 1 });
    const p = onLive.mock.calls[onLive.mock.calls.length - 1][0];
    expect(p.position.x).toBeCloseTo(0.6, 6);
    expect(p.position.y).toBeCloseTo(0.45, 6);
    expect(p.sizePct).toBeUndefined();
    // The box follows the drag before Rust answers.
    expect(box.style.left).toBe(`${((744 + 192) / 1920) * 100}%`);
    fireEvent.pointerUp(box, { pointerId: 1 });
    expect(onEnd).toHaveBeenCalledTimes(1);
  });

  it("a click without movement changes nothing", () => {
    const { onLive, onEnd } = mount();
    const box = screen.getByTestId("ed-camera-box");
    fireEvent.pointerDown(box, { clientX: 960, clientY: 540, pointerId: 1 });
    fireEvent.pointerMove(box, { clientX: 961, clientY: 541, pointerId: 1 });
    fireEvent.pointerUp(box, { pointerId: 1 });
    expect(onLive).not.toHaveBeenCalled();
    expect(onEnd).not.toHaveBeenCalled();
  });

  it("a corner handle resizes with the opposite corner fixed", () => {
    const { onLive, onEnd } = mount();
    const se = screen.getByTestId("ed-camera-handle-se");
    fireEvent.pointerDown(se, { clientX: 1176, clientY: 661.5, pointerId: 1 });
    fireEvent.pointerMove(se, { clientX: 1392, clientY: 700, pointerId: 1 });
    const p = onLive.mock.calls[onLive.mock.calls.length - 1][0];
    expect(p.sizePct).toBe(60);
    // Top-left fixed at (744, 418.5): centre = top-left + half of 648 × 364.5.
    expect(p.position.x).toBeCloseTo((744 + 324) / 1920, 6);
    expect(p.position.y).toBeCloseTo((418.5 + 182.25) / 1080, 6);
    fireEvent.pointerUp(se, { pointerId: 1 });
    expect(onEnd).toHaveBeenCalledTimes(1);
  });
});
