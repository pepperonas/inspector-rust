import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";

const recordPause = vi.fn();
const recordResume = vi.fn();
const recordStop = vi.fn();
const recordStatus = vi.fn();
let statusCb: ((e: { payload: unknown }) => void) | null = null;

vi.mock("../lib/ipc", () => ({
  recordPause: () => recordPause(),
  recordResume: () => recordResume(),
  recordStop: () => recordStop(),
  recordStatus: () => recordStatus(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_n: string, cb: (e: { payload: unknown }) => void) => {
    statusCb = cb;
    return () => undefined;
  },
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ setSize: async () => undefined }),
}));

import { RecordBar } from "./RecordBar";

const st = (phase: string, elapsedMs: number) => ({
  supported: true, reason: null, phase, elapsedMs, projectDir: "/x", source: "D", lowDiskMinutes: null,
});

beforeEach(() => {
  vi.useFakeTimers();
  recordStatus.mockResolvedValue(st("recording", 0));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("RecordBar", () => {
  it("pause calls recordPause, then shows Resume and freezes the clock", async () => {
    recordPause.mockResolvedValue(st("paused", 5000));
    const { getByText, getByTestId } = render(<RecordBar />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10);
    });
    await act(async () => {
      statusCb?.({ payload: st("recording", 5000) });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2300);
    });
    expect(getByTestId("rec-elapsed").textContent).toBe("0:07");

    await act(async () => {
      fireEvent.click(getByText("Pause"));
    });
    expect(recordPause).toHaveBeenCalledTimes(1);
    expect(getByText("Resume")).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(getByTestId("rec-elapsed").textContent).toBe("0:05");
  });
});
