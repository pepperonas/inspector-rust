import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";
import type { MeetingConfig, TickerConfig } from "../lib/ipc";

const handlers: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers[name] = cb;
    return () => {};
  }),
}));
let movedCb: (() => void) | null = null;
const startDragging = vi.fn(async () => {});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging,
    onMoved: (cb: () => void) => {
      movedCb = cb;
      return Promise.resolve(() => {});
    },
  }),
}));

const ticker: TickerConfig = {
  visible: true, position: "bottom", display: null, width_pct: 70, font_px: 15, opacity: 0.85,
  speed_px_s: 60, mode: "bar", place: null,
};
const cfg = (t: Partial<TickerConfig> = {}): MeetingConfig =>
  ({ ticker: { ...ticker, ...t } }) as unknown as MeetingConfig;
const tickerUpdate = vi.fn(async (p: { mode?: string; visible?: boolean }) => cfg({ mode: (p.mode ?? "bar") as TickerConfig["mode"] }));
vi.mock("../lib/ipc", () => ({
  getMeetingConfig: vi.fn(async () => cfg()),
  tickerUpdate: (p: never) => tickerUpdate(p),
}));

import { TranscribeTicker } from "./TranscribeTicker";

beforeEach(() => {
  tickerUpdate.mockClear();
  startDragging.mockClear();
});

async function hovered() {
  render(<TranscribeTicker />);
  await waitFor(() => expect(handlers["meeting-ticker-hover"]).toBeDefined());
  act(() => handlers["meeting-ticker-hover"]({ payload: true }));
}

describe("TranscribeTicker controls", () => {
  it("shows the controls only while the pointer is over the bar", async () => {
    render(<TranscribeTicker />);
    await waitFor(() => expect(handlers["meeting-ticker-hover"]).toBeDefined());
    expect(screen.queryByTestId("ticker-controls")).toBeNull();
    act(() => handlers["meeting-ticker-hover"]({ payload: true }));
    expect(screen.getByTestId("ticker-controls")).toBeTruthy();
    act(() => handlers["meeting-ticker-hover"]({ payload: false }));
    expect(screen.queryByTestId("ticker-controls")).toBeNull();
  });

  it("minimise, maximise and off go to the backend", async () => {
    await hovered();
    fireEvent.click(screen.getByTestId("ticker-max"));
    expect(tickerUpdate).toHaveBeenCalledWith({ mode: "max" });
    fireEvent.click(screen.getByTestId("ticker-off"));
    expect(tickerUpdate).toHaveBeenCalledWith({ visible: false });
    fireEvent.click(screen.getByTestId("ticker-mini"));
    expect(tickerUpdate).toHaveBeenCalledWith({ mode: "mini" });
  });

  it("saves the place once a drag comes to rest, not on every move", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await hovered();
    // A programmatic move (resize, display change) is not a drag.
    act(() => movedCb?.());
    act(() => vi.advanceTimersByTime(1000));
    expect(tickerUpdate).not.toHaveBeenCalled();
    fireEvent.pointerDown(screen.getByTestId("ticker-drag"));
    expect(startDragging).toHaveBeenCalled();
    act(() => movedCb?.());
    act(() => vi.advanceTimersByTime(100));
    act(() => movedCb?.());
    act(() => vi.advanceTimersByTime(500));
    expect(tickerUpdate).toHaveBeenCalledTimes(1);
    expect(tickerUpdate).toHaveBeenCalledWith({ save_place: true });
    vi.useRealTimers();
  });
});
