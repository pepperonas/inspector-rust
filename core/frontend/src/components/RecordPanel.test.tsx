// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";

const item = (over: Record<string, unknown>) => ({
  dir: "/p/a", title: "Demo", createdAt: 1_700_000_000_000, durationMs: 65_000,
  status: "ready", failure: null, active: false, ...over,
});
const m = vi.hoisted(() => ({
  status: { supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: null, lowDiskMinutes: null } as Record<string, unknown>,
  items: [] as Record<string, unknown>[],
  del: vi.fn(async () => {}),
}));
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => m.status,
  recordListProjects: async () => m.items,
  recordDeleteProject: m.del,
  recordRevealProject: async () => {},
  recordOpenHud: async () => {},
  recordStop: async () => "",
}));
import { RecordPanel } from "./RecordPanel";

async function mount(focused = true) {
  const r = render(<RecordPanel focused={focused} onRequestFocus={() => {}} onExit={() => {}} />);
  await act(async () => {});
  return r;
}

describe("RecordPanel", () => {
  beforeEach(() => {
    m.del.mockClear();
    m.status = { supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: null, lowDiskMinutes: null };
  });

  it("shows the reason of a failed project", async () => {
    m.items = [item({ status: "failed", failure: "Disk wurde voll" })];
    await mount();
    expect(screen.getByText("Disk wurde voll")).toBeTruthy();
  });

  it("marks the running take as recording now, not incomplete", async () => {
    m.items = [item({ status: "recording", active: true })];
    await mount();
    expect(screen.getByText("recording now")).toBeTruthy();
    expect(screen.queryByText("incomplete")).toBeNull();
  });

  it("delete arms on the first press and deletes on the second", async () => {
    m.items = [item({})];
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    expect(m.del).not.toHaveBeenCalled();
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    expect(m.del).toHaveBeenCalledWith("/p/a");
  });

  it("shows the reason instead of the list when unsupported", async () => {
    m.status = { ...m.status, supported: false, reason: "Recording needs macOS 14 or later." };
    m.items = [item({})];
    await mount();
    expect(screen.getByText("Recording needs macOS 14 or later.")).toBeTruthy();
    expect(screen.queryByText("Demo")).toBeNull();
  });
});
