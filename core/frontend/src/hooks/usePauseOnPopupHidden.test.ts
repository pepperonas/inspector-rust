import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, cleanup, act } from "@testing-library/react";

/** Deferred `listen` stub whose `unlisten` really detaches — see
 *  useTauriEvent.test.ts for the rationale. */
const { listen, pending, live, emit } = vi.hoisted(() => {
  interface Sub {
    event: string;
    handler: () => void;
    resolve: (u: () => void) => void;
  }
  const pending: Sub[] = [];
  const live = new Set<Sub>();
  const listen = vi.fn((event: string, handler: () => void) => {
    return new Promise<() => void>((resolve) => {
      pending.push({ event, handler, resolve });
    });
  });
  const emit = (event: string) => {
    for (const s of [...live]) if (s.event === event) s.handler();
  };
  return { listen, pending, live, emit };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { usePauseOnPopupHidden } from "./usePauseOnPopupHidden";

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

/** The popup was dismissed (focus loss → hide_popup). */
const hidePopup = () => act(() => emit("popup-hidden"));

afterEach(cleanup);
beforeEach(() => {
  listen.mockClear();
  pending.length = 0;
  live.clear();
});

describe("usePauseOnPopupHidden", () => {
  it("re-arms the resume gate when the popup hides mid-play", async () => {
    const engage = vi.fn();
    renderHook(() => usePauseOnPopupHidden(true, engage));
    await settle();

    expect(pending[0].event).toBe("popup-hidden");
    hidePopup();
    expect(engage).toHaveBeenCalledTimes(1);
  });

  it("stays quiet when the game is not in its playing phase", async () => {
    // Hiding the popup on the intro / game-over screen must not arm a gate
    // there is nothing to resume from.
    const engage = vi.fn();
    renderHook(() => usePauseOnPopupHidden(false, engage));
    await settle();

    hidePopup();
    expect(engage).not.toHaveBeenCalled();
  });

  it("reads the LATEST playing flag through refs without re-subscribing", async () => {
    const engage = vi.fn();
    const { rerender } = renderHook(
      ({ playing }) => usePauseOnPopupHidden(playing, engage),
      { initialProps: { playing: false } },
    );
    await settle();

    hidePopup();
    expect(engage).not.toHaveBeenCalled();

    rerender({ playing: true }); // the game started
    expect(listen).toHaveBeenCalledTimes(1); // still exactly ONE subscription
    expect(live.size).toBe(1);

    hidePopup();
    expect(engage).toHaveBeenCalledTimes(1);

    rerender({ playing: false }); // game over
    hidePopup();
    expect(engage).toHaveBeenCalledTimes(1); // no further gate
  });

  it("calls the LATEST engageGate, not the closure captured at mount", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(
      ({ engage }) => usePauseOnPopupHidden(true, engage),
      { initialProps: { engage: first } },
    );
    await settle();

    rerender({ engage: second });
    hidePopup();

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("detaches on unmount — including when listen() resolves afterwards", async () => {
    const engage = vi.fn();
    const { unmount } = renderHook(() => usePauseOnPopupHidden(true, engage));
    unmount(); // before the subscription settles
    await settle();

    expect(live.size).toBe(0);
    hidePopup();
    expect(engage).not.toHaveBeenCalled();
  });
});
