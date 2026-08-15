import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, cleanup, act } from "@testing-library/react";

/**
 * A faithful-enough `listen` stub: subscriptions resolve only when the test
 * says so (the whole point of this hook is the window between `listen()` being
 * called and its promise settling), and the returned `unlisten` REALLY detaches
 * — so "we cleaned up" is observable as "the handler stops receiving events"
 * rather than as a spy call count.
 */
const { listen, pending, live, emit } = vi.hoisted(() => {
  interface Sub {
    event: string;
    handler: (e: unknown) => void;
    resolve: (u: () => void) => void;
  }
  const pending: Sub[] = [];
  const live = new Set<Sub>();
  const listen = vi.fn((event: string, handler: (e: unknown) => void) => {
    return new Promise<() => void>((resolve) => {
      pending.push({ event, handler, resolve });
    });
  });
  /** Deliver an event to every still-attached listener. */
  const emit = (event: string, payload: unknown) => {
    for (const s of [...live]) if (s.event === event) s.handler({ event, payload });
  };
  return { listen, pending, live, emit };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { useTauriEvent } from "./useTauriEvent";

/** Settle the i-th outstanding `listen()`; its unlisten really detaches. */
async function settle(i = 0) {
  const sub = pending[i];
  const unlisten = vi.fn(() => void live.delete(sub));
  await act(async () => {
    live.add(sub);
    sub.resolve(unlisten);
    await Promise.resolve();
  });
  return unlisten;
}

afterEach(cleanup);
beforeEach(() => {
  listen.mockClear();
  pending.length = 0;
  live.clear();
});

describe("useTauriEvent", () => {
  it("subscribes once and routes payloads to the handler", async () => {
    const handler = vi.fn();
    renderHook(() => useTauriEvent("wakelock-changed", handler));

    expect(listen).toHaveBeenCalledTimes(1);
    expect(pending[0].event).toBe("wakelock-changed");

    await settle();
    act(() => emit("wakelock-changed", { on: true }));
    expect(handler).toHaveBeenCalledWith({ event: "wakelock-changed", payload: { on: true } });
  });

  it("stops receiving events after unmount", async () => {
    const handler = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent("foo", handler));
    await settle();

    unmount();
    act(() => emit("foo", 1));
    expect(handler).not.toHaveBeenCalled();
    expect(live.size).toBe(0);
  });

  it("REGRESSION: unmounting BEFORE listen() resolves still detaches the listener", async () => {
    // The documented leak this hook exists to prevent: the naive
    // `let unlisten; listen().then(u => unlisten = u); return () => unlisten?.()`
    // pattern runs its cleanup while `unlisten` is still undefined, orphaning
    // the listener for the lifetime of the app (in dev: every event fires
    // twice under the strict-mode double-mount).
    const handler = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent("foo", handler));
    unmount(); // <- the promise has NOT settled yet

    await settle();
    expect(live.size).toBe(0); // resolved straight into a cleanup

    act(() => emit("foo", 1));
    expect(handler).not.toHaveBeenCalled();
  });

  it("re-subscribes when deps change, leaving exactly one live listener", async () => {
    const handler = vi.fn();
    const { rerender } = renderHook(
      ({ dep }) => useTauriEvent("foo", handler, [dep]),
      { initialProps: { dep: 1 } },
    );
    await settle(0);

    rerender({ dep: 2 });
    expect(listen).toHaveBeenCalledTimes(2);
    await settle(1);

    expect(live.size).toBe(1); // the old one was torn down
    act(() => emit("foo", 1));
    expect(handler).toHaveBeenCalledTimes(1); // NOT delivered twice
  });

  it("does not re-subscribe when the deps are unchanged", async () => {
    const { rerender } = renderHook(() => useTauriEvent("foo", vi.fn(), []));
    await settle();
    rerender();
    rerender();
    expect(listen).toHaveBeenCalledTimes(1);
  });

  it("a stale subscription resolving after a deps change is cleaned up immediately", async () => {
    // The deps flip while the FIRST listen is still in flight — that promise
    // must not install a second live listener behind the new one's back.
    const handler = vi.fn();
    const { rerender } = renderHook(
      ({ dep }) => useTauriEvent("foo", handler, [dep]),
      { initialProps: { dep: 1 } },
    );
    rerender({ dep: 2 });

    await settle(0); // the stale one lands late
    await settle(1);

    expect(live.size).toBe(1);
    act(() => emit("foo", 1));
    expect(handler).toHaveBeenCalledTimes(1);
  });
});
