// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { FocusOverlay } from "./FocusOverlay";

afterEach(cleanup);

describe("FocusOverlay", () => {
  it("a click places the focus inside the screen box", () => {
    const onPick = vi.fn();
    render(<FocusOverlay rect={[0.1, 0.1, 0.8, 0.8]} focus={{ x: 0.5, y: 0.5 }} onPick={onPick} />);
    const el = screen.getByTestId("ed-focus-overlay");
    el.getBoundingClientRect = () => ({ left: 0, top: 0, width: 1000, height: 500, right: 1000, bottom: 500, x: 0, y: 0, toJSON: () => ({}) });
    fireEvent.pointerDown(el, { clientX: 500, clientY: 50 });
    expect(onPick).toHaveBeenCalledWith(0.5, 0);
  });
});
