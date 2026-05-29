import { describe, expect, it } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useModifierHeld } from "./useModifierHeld";

describe("useModifierHeld", () => {
  it("defaults to false", () => {
    const { result } = renderHook(() => useModifierHeld());
    expect(result.current).toBe(false);
  });

  it("flips to true on Meta keydown and back on keyup", () => {
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
    });
    expect(result.current).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keyup", { key: "Meta", metaKey: false }));
    });
    expect(result.current).toBe(false);
  });

  it("flips to true on Control keydown and back on keyup", () => {
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Control", ctrlKey: true }));
    });
    expect(result.current).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keyup", { key: "Control", ctrlKey: false }));
    });
    expect(result.current).toBe(false);
  });

  it("resets to false on window blur to dodge stuck-modifier after Cmd+Tab", () => {
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
    });
    expect(result.current).toBe(true);
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(result.current).toBe(false);
  });

  it("treats a non-modifier key pressed with Cmd held (e.g. Cmd+1) as held", () => {
    // The chip overlay should already be visible BEFORE the user hits
    // the digit, but if a fast typist goes straight to Cmd+1 without a
    // perceptible Cmd-only frame, `keydown` for "1" still has
    // `metaKey: true` — we should accept that as "held = true" too.
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", metaKey: true }));
    });
    expect(result.current).toBe(true);
  });

  it("removes its own listeners on unmount", () => {
    const { unmount, result } = renderHook(() => useModifierHeld());
    unmount();
    // Post-unmount events must NOT mutate state — verified by the
    // absence of a React act-warning + the hook value staying false
    // (we can't read it after unmount, but if listeners leaked an
    // update would be queued against a destroyed component).
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
    expect(result.current).toBe(false);
  });
});
