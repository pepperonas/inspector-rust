import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

const recordRegionChosen = vi.fn(async (_r: unknown) => undefined);
const recordRegionCancel = vi.fn(async () => undefined);
vi.mock("../lib/ipc", () => ({
  recordRegionChosen: (r: unknown) => recordRegionChosen(r),
  recordRegionCancel: () => recordRegionCancel(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RecordRegionPicker } from "./RecordRegionPicker";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const drag = (el: Element, a: [number, number], b: [number, number]) => {
  fireEvent.pointerDown(el, { button: 0, clientX: a[0], clientY: a[1], pointerId: 1 });
  fireEvent.pointerMove(el, { clientX: b[0], clientY: b[1], pointerId: 1 });
  fireEvent.pointerUp(el, { clientX: b[0], clientY: b[1], pointerId: 1 });
};

describe("RecordRegionPicker", () => {
  it("a drag makes an area that Enter hands to the backend", () => {
    const r = render(<RecordRegionPicker />);
    drag(r.getByTestId("rec-region-root"), [100, 50], [400, 250]);
    expect(r.getByTestId("rec-region-size").textContent).toBe("300 × 200");
    expect(r.getAllByTestId(/rec-region-handle-/)).toHaveLength(8);
    fireEvent.keyDown(window, { key: "Enter" });
    expect(recordRegionChosen).toHaveBeenCalledWith({ x: 100, y: 50, w: 300, h: 200 });
  });

  it("a click or a tiny drag is not an area", () => {
    const r = render(<RecordRegionPicker />);
    drag(r.getByTestId("rec-region-root"), [100, 100], [130, 300]);
    expect(r.queryByTestId("rec-region-rect")).toBeNull();
    fireEvent.keyDown(window, { key: "Enter" });
    expect(recordRegionChosen).not.toHaveBeenCalled();
  });

  it("a handle resizes and the body moves the area", () => {
    const r = render(<RecordRegionPicker />);
    drag(r.getByTestId("rec-region-root"), [100, 100], [300, 300]);
    drag(r.getByTestId("rec-region-handle-se"), [300, 300], [350, 320]);
    expect(r.getByTestId("rec-region-size").textContent).toBe("250 × 220");
    drag(r.getByTestId("rec-region-rect"), [150, 150], [160, 140]);
    fireEvent.click(r.getByRole("button", { name: "Use area" }));
    expect(recordRegionChosen).toHaveBeenCalledWith({ x: 110, y: 90, w: 250, h: 220 });
  });

  it("Esc cancels", () => {
    render(<RecordRegionPicker />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(recordRegionCancel).toHaveBeenCalledTimes(1);
  });
});
