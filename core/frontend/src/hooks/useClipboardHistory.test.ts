import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor, cleanup } from "@testing-library/react";

// The hook talks to two impure edges — the getHistory IPC and the Tauri event
// bus. Both are replaced: `handlers` is a manual event bus the tests fire
// into, exactly like the Rust side's `app.emit` would.
const { getHistory, handlers } = vi.hoisted(() => ({
  getHistory: vi.fn(async () => [] as unknown[]),
  handlers: new Map<string, () => void>(),
}));
vi.mock("../lib/ipc", () => ({ getHistory }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: () => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => handlers.delete(name));
  },
}));

import { useClipboardHistory } from "./useClipboardHistory";

afterEach(cleanup);
beforeEach(() => {
  handlers.clear();
  getHistory.mockClear();
});

/** Fire a Tauri event into the hook, flushing the resulting state updates. */
async function fire(name: string) {
  await act(async () => {
    handlers.get(name)?.();
  });
}

describe("useClipboardHistory — hidden-popup visibility gate (v0.105.0)", () => {
  it("loads once on mount, then SKIPS clipboard-changed while hidden", async () => {
    renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    expect(getHistory).toHaveBeenCalledTimes(1); // the initial load
    // The popup window is created hidden at launch — OS-wide copies must not
    // trigger a 1000-row refetch for a UI nobody can see.
    await fire("clipboard-changed");
    await fire("clipboard-changed");
    expect(getHistory).toHaveBeenCalledTimes(1);
  });

  it("window-shown refreshes unconditionally and re-enables live refetches", async () => {
    renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    await fire("window-shown");
    expect(getHistory).toHaveBeenCalledTimes(2); // fresh the moment it's visible
    await fire("clipboard-changed");
    expect(getHistory).toHaveBeenCalledTimes(3); // visible → live again
  });

  it("popup-hidden re-parks the refetch until the next show", async () => {
    renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    await fire("window-shown"); // 2
    await fire("popup-hidden");
    await fire("clipboard-changed");
    await fire("clipboard-changed");
    expect(getHistory).toHaveBeenCalledTimes(2); // parked again
    await fire("window-shown");
    expect(getHistory).toHaveBeenCalledTimes(3); // and fresh on re-open
  });

  it("unmount detaches all three listeners", async () => {
    const { unmount } = renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    unmount();
    expect(handlers.size).toBe(0);
  });
});
