import { describe, it, expect, vi, afterEach } from "vitest";
import { renderHook, cleanup } from "@testing-library/react";
import { useKeyboardNav } from "./useKeyboardNav";

afterEach(cleanup);

/** Dispatch a window keydown the hook will see. */
function press(key: string, opts: { shiftKey?: boolean } = {}) {
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key, shiftKey: opts.shiftKey ?? false, bubbles: true }),
  );
}

function setup(overrides: Partial<Parameters<typeof useKeyboardNav>[0]> = {}) {
  const setSelected = vi.fn();
  const onEnter = vi.fn();
  const onEscape = vi.fn();
  const onShiftArrow = vi.fn();
  renderHook(() =>
    useKeyboardNav({
      length: 10,
      selected: 3,
      setSelected,
      onEnter,
      onEscape,
      onShiftArrow,
      ...overrides,
    }),
  );
  return { setSelected, onEnter, onEscape, onShiftArrow };
}

describe("useKeyboardNav — list navigation", () => {
  it("ArrowDown moves the selection down by one", () => {
    const { setSelected } = setup();
    press("ArrowDown");
    expect(setSelected).toHaveBeenCalledWith(4);
  });

  it("ArrowUp moves the selection up by one", () => {
    const { setSelected } = setup();
    press("ArrowUp");
    expect(setSelected).toHaveBeenCalledWith(2);
  });

  it("ArrowDown clamps at the last index", () => {
    const { setSelected } = setup({ selected: 9, length: 10 });
    press("ArrowDown");
    expect(setSelected).toHaveBeenCalledWith(9);
  });

  it("ArrowUp clamps at index 0", () => {
    const { setSelected } = setup({ selected: 0 });
    press("ArrowUp");
    expect(setSelected).toHaveBeenCalledWith(0);
  });

  it("Enter calls onEnter with the shift flag", () => {
    const { onEnter } = setup();
    press("Enter");
    expect(onEnter).toHaveBeenCalledWith(false);
    press("Enter", { shiftKey: true });
    expect(onEnter).toHaveBeenCalledWith(true);
  });

  it("Escape calls onEscape", () => {
    const { onEscape } = setup();
    press("Escape");
    expect(onEscape).toHaveBeenCalledOnce();
  });
});

describe("useKeyboardNav — Shift+Arrow → volume", () => {
  it("Shift+ArrowUp calls onShiftArrow('up') and NOT setSelected", () => {
    const { onShiftArrow, setSelected } = setup();
    press("ArrowUp", { shiftKey: true });
    expect(onShiftArrow).toHaveBeenCalledWith("up");
    expect(setSelected).not.toHaveBeenCalled();
  });

  it("Shift+ArrowDown calls onShiftArrow('down') and NOT setSelected", () => {
    const { onShiftArrow, setSelected } = setup();
    press("ArrowDown", { shiftKey: true });
    expect(onShiftArrow).toHaveBeenCalledWith("down");
    expect(setSelected).not.toHaveBeenCalled();
  });

  it("plain (no-shift) arrows still navigate, never call onShiftArrow", () => {
    const { onShiftArrow, setSelected } = setup();
    press("ArrowUp");
    press("ArrowDown");
    expect(onShiftArrow).not.toHaveBeenCalled();
    expect(setSelected).toHaveBeenCalledTimes(2);
  });
});

describe("useKeyboardNav — enabled flag", () => {
  it("does nothing when enabled is false", () => {
    const { setSelected, onEnter, onEscape, onShiftArrow } = setup({ enabled: false });
    press("ArrowDown");
    press("Enter");
    press("Escape");
    press("ArrowUp", { shiftKey: true });
    expect(setSelected).not.toHaveBeenCalled();
    expect(onEnter).not.toHaveBeenCalled();
    expect(onEscape).not.toHaveBeenCalled();
    expect(onShiftArrow).not.toHaveBeenCalled();
  });
});
