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

  it("window-shown refreshes ONLY when a copy happened while hidden (B2), then goes live", async () => {
    // PERFORMANCE-PLAN B2 (v0.166.0): the pre-v0.166 contract refreshed on
    // EVERY open — a 1000-row decrypt + IPC for the common case of "no copy
    // since last time". A clean open is now free; a copy while visible still
    // refetches live.
    renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    await fire("window-shown");
    expect(getHistory).toHaveBeenCalledTimes(1); // nothing changed → no refetch
    await fire("clipboard-changed");
    expect(getHistory).toHaveBeenCalledTimes(2); // visible → live again
  });

  it("a copy while hidden marks the list stale, and the next show refreshes exactly once", async () => {
    renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    await fire("window-shown"); // clean → still 1
    await fire("popup-hidden");
    await fire("clipboard-changed");
    await fire("clipboard-changed");
    expect(getHistory).toHaveBeenCalledTimes(1); // parked: no fetch for a hidden UI
    await fire("window-shown");
    expect(getHistory).toHaveBeenCalledTimes(2); // stale → fresh on re-open
    await fire("popup-hidden");
    await fire("window-shown");
    expect(getHistory).toHaveBeenCalledTimes(2); // nothing new → free again
  });

  it("unmount detaches all three listeners", async () => {
    const { unmount } = renderHook(() => useClipboardHistory());
    await waitFor(() => expect(handlers.size).toBe(3));
    unmount();
    expect(handlers.size).toBe(0);
  });
});
