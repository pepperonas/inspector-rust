import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import type { MeetingConfig, MeetingModel, MeetingState } from "../lib/ipc";

const idle: MeetingState = {
  running: false, starting: false, dir: null, started_ms: null, sources: {}, lines: [], hypotheses: {},
  warnings: [], error: null, line_count: 0, download: null, recovered: [],
};
const cfg: MeetingConfig = {
  mic: true, sys: true, mic_device: null, model: "large-v3-turbo-q5", lang: "auto", root: null, keep_audio: true,
  audio_days: 7, dedupe: "auto", dedupe_threshold: 0.75,
  ticker: { visible: true, position: "bottom", display: null, width_pct: 70, font_px: 15, opacity: 0.85, speed_px_s: 60, mode: "bar", place: null },
};
let installed = true;
let state: MeetingState = idle;
const meetingStart = vi.fn(async () => {
  state = { ...idle, running: true, started_ms: Date.now() };
  return state;
});
const meetingStop = vi.fn(async () => {
  state = idle;
});
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  meetingStatus: vi.fn(async () => state),
  meetingModelStatus: vi.fn(async (): Promise<MeetingModel[]> => [
    { id: "large-v3-turbo-q5", file: "f", bytes: 574_041_195, installed },
  ]),
  meetingSessions: vi.fn(async () => []),
  getMeetingConfig: vi.fn(async () => cfg),
  meetingStart: () => meetingStart(),
  meetingStop: () => meetingStop(),
}));
vi.mock("../hooks/useTauriEvent", () => ({ useTauriEvent: () => undefined }));

import { MeetingPanel } from "./MeetingPanel";

describe("MeetingPanel", () => {
  beforeEach(() => {
    installed = true;
    state = idle;
    meetingStart.mockClear();
    meetingStop.mockClear();
  });

  it("Enter starts, the next Enter stops", async () => {
    const { rerender } = render(<MeetingPanel focused runSignal={100} onExit={() => {}} />);
    await waitFor(() => expect(meetingStart).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByTestId("meeting-toggle").textContent).toContain("Stop"));
    await act(async () => rerender(<MeetingPanel focused runSignal={101} onExit={() => {}} />));
    await waitFor(() => expect(meetingStop).toHaveBeenCalledTimes(1));
  });

  it("a remount does not replay the last Enter", async () => {
    render(<MeetingPanel focused runSignal={101} onExit={() => {}} />);
    await new Promise((r) => setTimeout(r, 30));
    expect(meetingStart).not.toHaveBeenCalled();
  });

  it("without the model it offers the download instead of starting", async () => {
    installed = false;
    render(<MeetingPanel focused runSignal={200} onExit={() => {}} />);
    await waitFor(() => expect(screen.getByTestId("meeting-model-card")).toBeTruthy());
    expect(screen.getByTestId("meeting-download").textContent).toContain("574 MB");
    expect(meetingStart).not.toHaveBeenCalled();
    expect((screen.getByTestId("meeting-toggle") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("MeetingPanel layout", () => {
  it("does not lay the sections out as a shrinking flex column", async () => {
    // Field report 2026-11-03: in a flex column the lines box shrank and its
    // text ran over "Show transcript folder" and the session list.
    const src = (await import("./MeetingPanel.tsx?raw")).default as string;
    const root = src.match(/data-testid="meeting-panel"/) ? src.slice(src.indexOf('<div className="'), src.indexOf('data-testid="meeting-panel"')) : "";
    expect(root).not.toMatch(/\bflex-col\b/);
    const box = src.slice(src.lastIndexOf('<div', src.indexOf('data-testid="meeting-lines"')), src.indexOf('data-testid="meeting-lines"'));
    expect(box).toMatch(/overflow-y-auto/);
    expect(box).toMatch(/max-h-/);
  });
});
