import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act, fireEvent } from "@testing-library/react";
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
  setMeetingConfig: (c: MeetingConfig) => setMeetingConfig(c),
  tickerUpdate: (p: unknown) => tickerUpdate(p),
}));
const tickerUpdate = vi.fn(async (_p: unknown) => cfg);
const setMeetingConfig = vi.fn(async (c: MeetingConfig) => c);
const handlers: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("../hooks/useTauriEvent", () => ({
  useTauriEvent: (name: string, fn: (e: { payload: unknown }) => void) => {
    handlers[name] = fn;
  },
}));

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

describe("MeetingPanel — what each event re-reads", () => {
  it("reads sessions only when a meeting ends and models only when a download finishes", async () => {
    const ipc = await import("../lib/ipc");
    render(<MeetingPanel focused={false} runSignal={0} onExit={() => {}} />);
    await waitFor(() => expect(handlers["meeting-changed"]).toBeTruthy());
    await waitFor(() => expect(ipc.meetingSessions).toHaveBeenCalled());
    const sessions = vi.mocked(ipc.meetingSessions);
    const models = vi.mocked(ipc.meetingModelStatus);
    sessions.mockClear();
    models.mockClear();
    const fire = (p: Partial<MeetingState>) => act(async () => handlers["meeting-changed"]({ payload: { ...idle, ...p } }));
    // A running meeting sends many events: nothing is re-read.
    for (let i = 0; i < 5; i++) await fire({ running: true, line_count: i });
    expect(sessions).not.toHaveBeenCalled();
    expect(models).not.toHaveBeenCalled();
    // It ends: the session list once.
    await fire({});
    await fire({});
    expect(sessions).toHaveBeenCalledTimes(1);
    // A download reports progress many times, then finishes: the model list once.
    const download = { model: "large-v3-turbo-q5", done: 1, total: 2 } as unknown as MeetingState["download"];
    for (let i = 0; i < 4; i++) await fire({ download });
    expect(models).not.toHaveBeenCalled();
    await fire({ download: null });
    expect(models).toHaveBeenCalledTimes(1);
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

describe("MeetingPanel language", () => {
  it("switches the language from the panel, also while recording", async () => {
    render(<MeetingPanel focused={false} runSignal={0} onExit={() => undefined} />);
    const en = await screen.findByTestId("lang-en");
    fireEvent.click(en);
    await waitFor(() => expect(setMeetingConfig).toHaveBeenCalled());
    expect(setMeetingConfig.mock.calls[setMeetingConfig.mock.calls.length - 1][0].lang).toBe("en");
  });
});

describe("MeetingPanel ticker control", () => {
  it("shows a closed ticker as off and opens it again from the panel", async () => {
    const hidden = { ...cfg, ticker: { ...cfg.ticker, visible: false } };
    tickerUpdate.mockResolvedValueOnce({ ...cfg, ticker: { ...cfg.ticker, visible: true, mode: "bar" } });
    const { getMeetingConfig } = await import("../lib/ipc");
    vi.mocked(getMeetingConfig).mockResolvedValueOnce(hidden);
    render(<MeetingPanel focused={false} runSignal={0} onExit={() => undefined} />);
    const off = await screen.findByTestId("ticker-choice-off");
    expect(off.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByTestId("ticker-choice-bar"));
    await waitFor(() => expect(tickerUpdate).toHaveBeenCalledWith({ visible: true, mode: "bar" }));
    await waitFor(() => expect(screen.getByTestId("ticker-choice-bar").getAttribute("aria-pressed")).toBe("true"));
  });

  it("follows a change made from the ticker itself", async () => {
    render(<MeetingPanel focused={false} runSignal={0} onExit={() => undefined} />);
    await screen.findByTestId("ticker-choice-bar");
    act(() => handlers["meeting-config-changed"]({ payload: { ...cfg, ticker: { ...cfg.ticker, visible: false } } }));
    expect(screen.getByTestId("ticker-choice-off").getAttribute("aria-pressed")).toBe("true");
  });

  it("offers docking only when the ticker was dragged away", async () => {
    render(<MeetingPanel focused={false} runSignal={0} onExit={() => undefined} />);
    await screen.findByTestId("ticker-choice-bar");
    expect(screen.queryByTestId("ticker-dock")).toBeNull();
    act(() => handlers["meeting-config-changed"]({ payload: { ...cfg, ticker: { ...cfg.ticker, place: [0.2, 0.3] } } }));
    fireEvent.click(screen.getByTestId("ticker-dock"));
    await waitFor(() => expect(tickerUpdate).toHaveBeenCalledWith({ reset_place: true }));
  });
});
