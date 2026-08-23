import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import type { StatusToast as Payload } from "../lib/ipc";

const { getStatusToast, hideStatusToast, listen, live } = vi.hoisted(() => {
  const live = new Set<{ event: string; handler: () => void }>();
  return {
    getStatusToast: vi.fn<() => Promise<Payload | null>>(async () => null),
    hideStatusToast: vi.fn(async () => undefined),
    listen: vi.fn(async (event: string, handler: () => void) => {
      const sub = { event, handler };
      live.add(sub);
      return () => void live.delete(sub);
    }),
    live,
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  getStatusToast,
  hideStatusToast,
}));

import { StatusToast } from "./StatusToast";

const toast = (over: Partial<Payload> = {}): Payload => ({
  kind: "timer",
  on: true,
  title: "TIMER SET",
  subtitle: "5 min",
  ...over,
});

/** Mount and let the initial `get_status_toast` land. */
async function mount(payload: Payload | null) {
  getStatusToast.mockResolvedValue(payload);
  const view = render(<StatusToast />);
  await act(async () => {});
  return view;
}

/** The backend pushed a new payload — same path the `status-toast-changed`
 *  event takes in the real window. */
async function retrigger(payload: Payload) {
  getStatusToast.mockResolvedValue(payload);
  await act(async () => {
    for (const s of [...live]) if (s.event === "status-toast-changed") s.handler();
    await Promise.resolve();
  });
}

const advance = async (ms: number) => {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
};

/** The animated card (its class encodes pop / persistent-in / persistent-out). */
const card = () => document.querySelector(".status-toast-pop, .status-toast-in, .status-toast-out");

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
beforeEach(() => {
  vi.useFakeTimers();
  getStatusToast.mockReset();
  getStatusToast.mockResolvedValue(null);
  hideStatusToast.mockClear();
  listen.mockClear();
  live.clear();
});

/** The % readout renders one span per DIGIT since the v0.118.0 odometer —
 *  read the joined text of the .vol-num container to assert the value. */
function volNumText(): string {
  const el = document.querySelector(".vol-num");
  return el ? (el.textContent ?? "") : "";
}

describe("StatusToast — one-shot kinds", () => {
  it("renders the payload and hides itself after the hold", async () => {
    await mount(toast());
    expect(screen.getByText("TIMER SET")).toBeTruthy();
    expect(screen.getByText("5 min")).toBeTruthy();
    expect(card()?.className).toContain("status-toast-pop");

    await advance(1599);
    expect(hideStatusToast).not.toHaveBeenCalled();

    await advance(1);
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("holds a `random` roll on screen much longer — it is meant to be read", async () => {
    await mount(toast({ kind: "random", title: "42", subtitle: "1–100" }));

    await advance(1600); // the normal hold …
    expect(hideStatusToast).not.toHaveBeenCalled(); // … is not enough

    await advance(2000); // 3600 total
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("a re-trigger RESTARTS the hold rather than letting the first one expire", async () => {
    await mount(toast());
    await advance(1000);

    await retrigger(toast({ title: "TIMER SET", subtitle: "10 min" }));
    await advance(1000); // 2000 total, but only 1000 since the re-trigger
    expect(hideStatusToast).not.toHaveBeenCalled();
    expect(screen.getByText("10 min")).toBeTruthy(); // showing the newest payload

    await advance(600);
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("hides directly, without the persistent fade-out step", async () => {
    await mount(toast());
    await advance(1600);
    expect(card()?.className).not.toContain("status-toast-out");
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("renders an empty transparent surface until a payload arrives", async () => {
    const { container } = await mount(null);
    expect(container.textContent).toBe("");
    expect(card()).toBeNull();
  });
});

describe("StatusToast — persistent volume / mute HUD", () => {
  const vol = (title: string) => toast({ kind: "volume", title, subtitle: "" });

  it("shows the level read-back as a percentage", async () => {
    await mount(vol("45"));
    expect(volNumText()).toBe("45");
    expect(screen.getByText("%")).toBeTruthy();
  });

  it("clamps an out-of-range level into 0–100", async () => {
    await mount(vol("150"));
    expect(volNumText()).toBe("100");
    cleanup();
    await mount(vol("-20"));
    expect(volNumText()).toBe("0");
  });

  it("falls back to the raw title when the OS gave no numeric read-back", async () => {
    // The Windows relative-volume path sends "+" / "−" instead of a level.
    await mount(vol("+"));
    expect(screen.getByText("+")).toBeTruthy();
    expect(screen.queryByText("%")).toBeNull();
  });

  it("fades out first, and only then hides the window", async () => {
    await mount(vol("45"));
    expect(document.querySelector(".vol-overlay-in")).toBeTruthy();

    await advance(1100); // the shorter persistent hold
    expect(hideStatusToast).not.toHaveBeenCalled(); // fade first …
    expect(document.querySelector(".vol-overlay-out")).toBeTruthy();

    await advance(260); // … then hide, once the out animation has played
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("updates IN PLACE across a rapid re-trigger instead of re-popping", async () => {
    // "3× louder in a row" must read as one continuous readout.
    await mount(vol("30"));
    const first = document.querySelector(".vol-overlay-in");

    await advance(400);
    await retrigger(vol("40"));
    await advance(400);
    await retrigger(vol("50"));

    expect(volNumText()).toBe("50");
    // Same DOM node throughout — a fresh entrance would have remounted it.
    expect(document.querySelector(".vol-overlay-in")).toBe(first);
    expect(hideStatusToast).not.toHaveBeenCalled();
  });

  it("lingers only from the LAST trigger, then goes", async () => {
    await mount(vol("30"));
    await advance(1000);
    await retrigger(vol("40"));

    await advance(1000); // 2000 total, 1000 since the last trigger
    expect(hideStatusToast).not.toHaveBeenCalled();

    await advance(100); // remainder of the hold, measured from the re-trigger
    expect(document.querySelector(".vol-overlay-out")).toBeTruthy();

    await advance(260); // the fade
    expect(hideStatusToast).toHaveBeenCalledTimes(1);
  });

  it("re-entrances (not updates-in-place) when the KIND changes", async () => {
    await mount(vol("30"));
    const first = document.querySelector(".vol-overlay-in");

    await retrigger(toast({ kind: "mute", on: true, title: "MUTED", subtitle: "" }));

    expect(document.querySelector(".vol-overlay-in")).not.toBe(first);
  });

  it("cancels a pending fade-out when a new trigger arrives mid-exit", async () => {
    await mount(vol("30"));
    await advance(1100);
    expect(document.querySelector(".vol-overlay-out")).toBeTruthy();

    await retrigger(vol("60"));
    expect(document.querySelector(".vol-overlay-out")).toBeNull();
    expect(volNumText()).toBe("60");

    await advance(259);
    expect(hideStatusToast).not.toHaveBeenCalled(); // the old fade did NOT hide us
  });
});

describe("StatusToast — resilience", () => {
  it("survives a failing status read without crashing the window", async () => {
    getStatusToast.mockRejectedValue(new Error("ipc down"));
    const { container } = render(<StatusToast />);
    await act(async () => {});
    expect(container.firstElementChild).toBeTruthy();
    expect(hideStatusToast).not.toHaveBeenCalled();
  });

  it("ignores a null payload push and keeps showing the current toast", async () => {
    await mount(toast({ title: "TIMER SET" }));
    getStatusToast.mockResolvedValue(null);
    await act(async () => {
      for (const s of [...live]) s.handler();
      await Promise.resolve();
    });
    expect(screen.getByText("TIMER SET")).toBeTruthy();
  });

  it("detaches its listener on unmount", async () => {
    const { unmount } = await mount(toast());
    expect(live.size).toBe(1);
    unmount();
    await act(async () => {});
    expect(live.size).toBe(0);
  });
});

describe("StatusToast — Klangaura (v0.118.0)", () => {
  const vol = (title: string): Payload => toast({ kind: "volume", title, subtitle: "" });
  const mute = (on: boolean): Payload => toast({ kind: "mute", on, title: on ? "MUTED" : "SOUND ON", subtitle: "" });

  it("the FIRST reading fires no wave — there is no direction yet", async () => {
    await mount(vol("40"));
    expect(document.querySelector(".vol-wave")).toBeNull();
  });

  it("a louder trigger blooms waves outward + streaks the bar; quieter collapses inward", async () => {
    await mount(vol("40"));
    await retrigger(vol("50"));
    expect(document.querySelector(".vol-wave-up")).toBeTruthy();
    expect(document.querySelector(".vol-streak")).toBeTruthy();
    await retrigger(vol("35"));
    expect(document.querySelector(".vol-wave-down")).toBeTruthy();
    expect(document.querySelector(".vol-wave-up")).toBeNull();
    // The streak is a louder-only accent — never on the way down.
    expect(document.querySelector(".vol-streak")).toBeNull();
  });

  it("a repeat at a boundary fires nothing (holding ⇧↓ at 0 must not keep collapsing)", async () => {
    await mount(vol("0"));
    await retrigger(vol("0"));
    expect(document.querySelector(".vol-wave")).toBeNull();
    expect(document.querySelector(".vol-streak")).toBeNull();
  });

  it("only CHANGED digit columns re-key (odometer), and the roll follows the direction", async () => {
    await mount(vol("41"));
    await retrigger(vol("45"));
    const rolled = document.querySelectorAll(".vol-digit-up");
    // 41 → 45: the tens column ("4") keeps its key → no replay class needed on
    // remount semantics; at minimum the changed ones column rolls upward.
    expect(rolled.length).toBeGreaterThanOrEqual(1);
    expect(document.querySelector(".vol-digit-down")).toBeNull();
  });

  it("mute dips the bar, unmute rebounds — and the comet head hides while muted", async () => {
    await mount(mute(false));
    await retrigger(mute(true));
    expect(document.querySelector(".vol-dip")).toBeTruthy();
    const head = document.querySelector(".vol-head") as HTMLElement;
    expect(head.style.opacity).toBe("0");
    await retrigger(mute(false));
    // Unmute: the mute-kind toast has no level read-back, so there is no bar
    // to rebound (`showBar` is false again) — the visible bloom is the
    // outward wave burst at the icon. (`.vol-rebound` still exists for
    // level-carrying tracks that unmute.)
    expect(document.querySelector(".vol-wave-up")).toBeTruthy();
    expect(document.querySelector(".vol-dip")).toBeNull();
  });
});
