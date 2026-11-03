// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordCropRect } from "../../lib/ipc";
import { CameraFraming } from "./CameraFraming";

afterEach(cleanup);

const half: RecordCropRect = { x: 0.25, y: 0.25, w: 0.5, h: 0.5 };

function mount(crop: RecordCropRect | null) {
  const onLive = vi.fn();
  const onEnd = vi.fn();
  const onCommit = vi.fn();
  render(<CameraFraming src={null} crop={crop} onLive={onLive} onEnd={onEnd} onCommit={onCommit} />);
  // The still measures 200×100 px (happy-dom has no layout).
  const box = screen.getByTestId("ed-camera-framing");
  box.getBoundingClientRect = () => ({ left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) });
  return { onLive, onEnd, onCommit, box };
}
const last = (f: ReturnType<typeof vi.fn>) => f.mock.calls[f.mock.calls.length - 1]?.[0] as RecordCropRect | null;

describe("camera framing", () => {
  it("dragging the frame pans live and becomes one undo step on release", () => {
    const { onLive, onEnd, onCommit } = mount(half);
    const frame = screen.getByTestId("ed-camera-frame");
    fireEvent.pointerDown(frame, { clientX: 100, clientY: 50, pointerId: 1 });
    // 50 px of the 100 px the frame can travel: pan 0.5 → 1.
    fireEvent.pointerMove(frame, { clientX: 150, clientY: 50, pointerId: 1 });
    expect(last(onLive)).toEqual({ x: 0.5, y: 0.25, w: 0.5, h: 0.5 });
    expect(onEnd).not.toHaveBeenCalled();
    fireEvent.pointerUp(frame, { clientX: 150, clientY: 50, pointerId: 1 });
    expect(onEnd).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("arrow keys nudge by 5 % and + zooms in, each one step; the editor never sees the key", () => {
    const { onCommit, box } = mount(half);
    const ev = new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true });
    box.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect(last(onCommit)?.x).toBeCloseTo(0.225);
    fireEvent.keyDown(box, { key: "+" });
    expect(last(onCommit)?.w).toBeCloseTo(0.5 / 1.1);
    expect(onCommit).toHaveBeenCalledTimes(2);
  });

  it("at the whole picture there is nothing to pan", () => {
    const { onLive, onCommit, box } = mount(null);
    const frame = screen.getByTestId("ed-camera-frame");
    fireEvent.pointerDown(frame, { clientX: 100, clientY: 50, pointerId: 1 });
    fireEvent.pointerMove(frame, { clientX: 150, clientY: 50, pointerId: 1 });
    expect(onLive).not.toHaveBeenCalled();
    fireEvent.keyDown(box, { key: "ArrowLeft" });
    expect(onCommit).not.toHaveBeenCalled();
  });
});
