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
  open: vi.fn(async (): Promise<void> => {}),
}));
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => m.status,
  recordListProjects: async () => m.items,
  recordDeleteProject: m.del,
  recordRevealProject: async () => {},
  recordOpenHud: m.open,
  recordStop: async () => "",
}));
import { RecordPanel } from "./RecordPanel";

async function mount(focused = true, openSignal = 0) {
  const r = render(
    <RecordPanel focused={focused} openSignal={openSignal} onRequestFocus={() => {}} onExit={() => {}} />,
  );
  await act(async () => {});
  return r;
}

describe("RecordPanel", () => {
  beforeEach(() => {
    m.del.mockClear();
    m.open.mockReset();
    m.open.mockImplementation(async () => {});
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

  it("Enter while a delete is armed backs out instead of starting a recording", async () => {
    m.items = [item({})];
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    expect(screen.getByText("Delete the project?")).toBeTruthy();
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.open).not.toHaveBeenCalled();
    expect(m.del).not.toHaveBeenCalled();
    expect(screen.queryByText("Delete the project?")).toBeNull();
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.open).toHaveBeenCalledTimes(1);
  });

  it("keeps the selection on a real row when the list shrinks", async () => {
    m.items = [item({ dir: "/p/a", title: "A" }), item({ dir: "/p/b", title: "B" }), item({ dir: "/p/c", title: "C" })];
    const r = await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "ArrowDown" }); });
    await act(async () => { fireEvent.keyDown(window, { key: "ArrowDown" }); });
    m.items = [item({ dir: "/p/a", title: "A" })];
    r.rerender(<RecordPanel focused openSignal={0} onRequestFocus={() => {}} onExit={() => {}} />);
    // Refresh comes from the poll; drive it by remounting the data.
    await act(async () => { await new Promise((res) => setTimeout(res, 1600)); });
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    expect(m.del).toHaveBeenCalledWith("/p/a");
  });

  it("selection never leaves the rendered rows", async () => {
    m.items = Array.from({ length: 25 }, (_, i) => item({ dir: `/p/${i}`, title: `T${i}` }));
    await mount();
    for (let i = 0; i < 30; i++) {
      await act(async () => { fireEvent.keyDown(window, { key: "ArrowDown" }); });
    }
    expect(screen.queryByText("T20")).toBeNull();
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
    expect(m.del).toHaveBeenCalledWith("/p/19");
  });

  it("a record dispatch while a take runs explains it in the panel", async () => {
    m.status = { ...m.status, phase: "recording" };
    m.items = [];
    m.open.mockImplementation(async () => { throw "record.busy"; });
    await mount(false, 101);
    expect(m.open).toHaveBeenCalledTimes(1);
    expect(screen.getByText("A recording is already running.")).toBeTruthy();
  });

  it("the same dispatch signal is not replayed by a remount", async () => {
    m.items = [];
    const r = await mount(false, 202);
    expect(m.open).toHaveBeenCalledTimes(1);
    r.unmount();
    await mount(false, 202);
    expect(m.open).toHaveBeenCalledTimes(1);
  });
});
