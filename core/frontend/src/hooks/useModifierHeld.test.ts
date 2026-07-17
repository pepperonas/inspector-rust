import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { useModifierHeld } from "./useModifierHeld";

// Without `globals: true` testing-library can't auto-register its cleanup, so
// every React test file must do it itself (the sibling hook tests all do).
// This file missing it was the intermittent "window is not defined" crash:
// the un-unmounted tree left a React scheduler tick pending past happy-dom's
// environment teardown.
afterEach(cleanup);

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
    // Post-unmount events must NOT mutate state. act-wrapped so that a
    // leaked listener's setState would flush synchronously and fail the
    // assertion (un-wrapped, the stale snapshot could pass despite a leak).
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
    });
    expect(result.current).toBe(false);
  });

  it("treats Ctrl-combos (e.g. Ctrl+1 on Win/Linux) as held too", () => {
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", ctrlKey: true }));
    });
    expect(result.current).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keyup", { key: "Control", ctrlKey: false }));
    });
    expect(result.current).toBe(false);
  });

  it("ignores plain non-modifier keys", () => {
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Shift", shiftKey: true }));
    });
    expect(result.current).toBe(false);
  });

  it("a non-modifier keyup never clears a held modifier", () => {
    // Cmd stays held while the user releases the digit: keyup "1" must not
    // hide the overlay — only releasing Meta/Control (or blur) does.
    const { result } = renderHook(() => useModifierHeld());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
      window.dispatchEvent(new KeyboardEvent("keyup", { key: "1", metaKey: true }));
    });
    expect(result.current).toBe(true);
  });
});
