import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LogiConfig, LogiStatus, LogiView } from "../lib/logi";

const status: LogiStatus = {
  supported: true,
  connected: true,
  permission_denied: false,
  name: "MX Master",
  battery: { percent: 90, charging: false },
  dpi: { current: 1000, default: 1000, options: [400, 1000, 1600] },
  wheel: { mode: "smart", threshold: 16 },
  invert: false,
  controls: [
    { cid: 0x50, divertable: false },
    { cid: 0x53, divertable: true },
    { cid: 0xc3, divertable: true },
  ],
  options_running: false,
};
const empty: LogiConfig = { dpi: null, wheel: null, invert: null, buttons: {} };

const ipc = vi.hoisted(() => ({
  logiView: vi.fn(),
  logiRefresh: vi.fn(),
  logiSetConfig: vi.fn(),
  logiRequestAccess: vi.fn(),
  listActionHotkeys: vi.fn(),
  aiTasksState: vi.fn(),
}));
vi.mock("../lib/ipc", () => ipc);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { LogiPanel, SAVE_DEBOUNCE_MS } from "./LogiPanel";

function view(s: Partial<LogiStatus> = {}, c: LogiConfig = empty): LogiView {
  return { status: { ...status, ...s }, config: c };
}

async function open(v: LogiView = view()) {
  ipc.logiView.mockResolvedValue(v);
  ipc.logiRefresh.mockResolvedValue(v);
  ipc.logiSetConfig.mockImplementation((config: LogiConfig) => Promise.resolve({ status: v.status, config }));
  ipc.listActionHotkeys.mockResolvedValue([]);
  ipc.aiTasksState.mockResolvedValue({ tasks: [] });
  ipc.logiRequestAccess.mockResolvedValue(true);
  render(<LogiPanel focused onExit={() => {}} />);
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function settle() {
  await act(async () => {
    vi.advanceTimersByTime(SAVE_DEBOUNCE_MS + 10);
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  Object.values(ipc).forEach((f) => f.mockReset());
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("LogiPanel", () => {
  it("shows the mouse, its battery and the DPI it runs at", async () => {
    await open();
    expect(screen.getByText("MX Master")).toBeTruthy();
    expect(screen.getByText("90 %")).toBeTruthy();
    expect(screen.getByRole("button", { name: /^1000/ }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "1600" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("lists only the buttons that can take an action", async () => {
    await open();
    expect(screen.getByText("Thumb button")).toBeTruthy();
    expect(screen.getByText("Back")).toBeTruthy();
    expect(screen.queryByText("Left button")).toBeNull();
  });

  it("saves a DPI choice after the pause, once", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "1600" }));
    expect(ipc.logiSetConfig).not.toHaveBeenCalled();
    await settle();
    expect(ipc.logiSetConfig).toHaveBeenCalledTimes(1);
    expect(ipc.logiSetConfig.mock.calls[0][0].dpi).toBe(1600);
  });

  it("saves only the last of several quick changes", async () => {
    await open();
    const slider = screen.getByRole("slider");
    fireEvent.change(slider, { target: { value: "20" } });
    fireEvent.change(slider, { target: { value: "30" } });
    await settle();
    expect(ipc.logiSetConfig).toHaveBeenCalledTimes(1);
    expect(ipc.logiSetConfig.mock.calls[0][0].wheel).toEqual({ mode: "smart", threshold: 30 });
  });

  it("gives a button an action", async () => {
    await open();
    fireEvent.change(screen.getByLabelText("Back: press"), { target: { value: "prev_tab" } });
    await settle();
    expect(ipc.logiSetConfig.mock.calls[0][0].buttons).toEqual({ "83": { press: { type: "prev_tab" } } });
  });

  it("holds back an action that still misses its target", async () => {
    await open();
    fireEvent.change(screen.getByLabelText("Back: press"), { target: { value: "open" } });
    await settle();
    expect(ipc.logiSetConfig).not.toHaveBeenCalled();
    expect(screen.getByText(/missing its target/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Address or path"), { target: { value: "https://example.com" } });
    await settle();
    expect(ipc.logiSetConfig).toHaveBeenCalledTimes(1);
  });

  it("opens the direction rows of a button that has direction actions", async () => {
    await open(view({}, { ...empty, buttons: { "195": { up: { type: "volume_up" } } } }));
    expect((screen.getByLabelText("Thumb button: Move up") as HTMLSelectElement).value).toBe("volume_up");
    expect(screen.queryByLabelText("Back: Move up")).toBeNull();
  });

  it("says so when no mouse is connected, and offers nothing to set", async () => {
    await open(view({ connected: false, name: "", battery: null, dpi: null, wheel: null, invert: null, controls: [] }));
    expect(screen.getByText("No mouse connected")).toBeTruthy();
    expect(screen.queryByRole("slider")).toBeNull();
  });

  it("warns while Logi Options+ runs and asks for access when macOS refuses", async () => {
    await open(view({ options_running: true, permission_denied: true }));
    expect(screen.getByText(/Logi Options\+ is running/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Allow access…" }));
    expect(ipc.logiRequestAccess).toHaveBeenCalledTimes(1);
  });

  it("says that macOS blocks a mouse it found, instead of 'connect a mouse'", async () => {
    await open(view({ connected: false, refused: true, name: "MX Master" }));
    const box = screen.getByTestId("logi-refused");
    expect(box.textContent).toContain("MX Master found");
    expect(box.textContent).toContain("keeps trying");
    expect(screen.queryByText(/Connect a Logitech mouse/)).toBeNull();
  });

  it("names Secure Input as the reason when it is on", async () => {
    await open(view({ connected: false, refused: true, secure_input: true, name: "MX Master" }));
    expect(screen.getByTestId("logi-refused").textContent).toContain("Secure Input is on");
  });

  it("leaves on Escape", async () => {
    const onExit = vi.fn();
    ipc.logiView.mockResolvedValue(view());
    ipc.logiRefresh.mockResolvedValue(view());
    ipc.listActionHotkeys.mockResolvedValue([]);
    ipc.aiTasksState.mockResolvedValue({ tasks: [] });
    render(<LogiPanel focused onExit={onExit} />);
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).toHaveBeenCalledTimes(1);
  });
});
