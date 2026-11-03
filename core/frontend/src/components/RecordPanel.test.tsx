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
  edit: vi.fn(async (_dir: string): Promise<unknown> => ({})),
  rename: vi.fn(async (_dir: string, title: string): Promise<string> => title.trim()),
  pick: vi.fn(async (): Promise<string | null> => null),
}));
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => m.status,
  recordListProjects: async () => m.items,
  recordDeleteProject: m.del,
  recordRevealProject: async () => {},
  recordOpenHud: m.open,
  recordStop: async () => "",
  editorOpen: m.edit,
  recordRenameProject: m.rename,
  recordOpenDialog: m.pick,
}));
import { RecordPanel } from "./RecordPanel";

async function mount(focused = true, openSignal = 0, query = "") {
  const r = render(
    <RecordPanel focused={focused} query={query} openSignal={openSignal} onRequestFocus={() => {}} onExit={() => {}} />,
  );
  await act(async () => {});
  return r;
}

describe("RecordPanel", () => {
  beforeEach(() => {
    m.del.mockClear();
    m.open.mockReset();
    m.open.mockImplementation(async () => {});
    m.edit.mockReset();
    m.edit.mockImplementation(async () => ({}));
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
    // Stage 2: Enter on the selected project opens it in the editor (a new
    // recording stays on the button / `record` without a selection).
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.open).not.toHaveBeenCalled();
    expect(m.edit).toHaveBeenCalledWith("/p/a");
  });

  it("Enter on the selected project opens it in the editor", async () => {
    m.items = [item({ dir: "/p/a", title: "A" }), item({ dir: "/p/b", title: "B" })];
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "ArrowDown" }); });
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.edit).toHaveBeenCalledWith("/p/b");
    expect(m.open).not.toHaveBeenCalled();
  });

  it("a failed or running project does not open the editor", async () => {
    m.items = [item({ status: "failed", failure: "kaputt" })];
    const r = await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.edit).not.toHaveBeenCalled();
    expect(m.open).not.toHaveBeenCalled();
    r.unmount();
    m.items = [item({ status: "recording", active: true })];
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.edit).not.toHaveBeenCalled();
  });

  it("the Edit button opens the editor; an editor error is shown in words", async () => {
    m.items = [item({})];
    m.edit.mockImplementation(async () => { throw "record.editor.no_video"; });
    await mount(false);
    await act(async () => { fireEvent.click(screen.getByText("Edit")); });
    expect(m.edit).toHaveBeenCalledWith("/p/a");
    expect(screen.getByText("This recording has no readable video (screen.mp4).")).toBeTruthy();
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

  it("lists every project, not just 20 — the list scrolls and the selection follows", async () => {
    m.items = Array.from({ length: 25 }, (_, i) => item({ dir: `/p/${i}`, title: `T${i}` }));
    const scrolled: string[] = [];
    const orig = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = function () {
      scrolled.push(this.textContent ?? "");
    };
    try {
      await mount();
      expect(screen.getAllByTestId("record-row")).toHaveLength(25);
      expect(screen.getByText("T24")).toBeTruthy();
      for (let i = 0; i < 30; i++) {
        await act(async () => { fireEvent.keyDown(window, { key: "ArrowDown" }); });
      }
      expect(scrolled[scrolled.length - 1]).toContain("T24");
      await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
      await act(async () => { fireEvent.keyDown(window, { key: "Backspace" }); });
      expect(m.del).toHaveBeenCalledWith("/p/24");
    } finally {
      HTMLElement.prototype.scrollIntoView = orig;
    }
  });

  it("the search filters by name (accent- and case-insensitive) and by date", async () => {
    m.items = [
      item({ dir: "/p/a", title: "Überblick Q3", createdAt: Date.parse("2026-03-14T10:00:00") }),
      item({ dir: "/p/b", title: "Demo", createdAt: Date.parse("2026-05-02T10:00:00") }),
    ];
    const r = await mount(true, 0, "uberblick");
    expect(screen.getAllByTestId("record-row")).toHaveLength(1);
    expect(screen.getByText("Überblick Q3")).toBeTruthy();
    r.unmount();
    await mount(true, 0, "2026-05-02");
    expect(screen.getAllByTestId("record-row")).toHaveLength(1);
    expect(screen.getByText("Demo")).toBeTruthy();
    // Enter acts on the filtered row.
    await act(async () => { fireEvent.keyDown(window, { key: "Enter" }); });
    expect(m.edit).toHaveBeenCalledWith("/p/b");
  });

  it("a search without a match says so", async () => {
    m.items = [item({ title: "Demo" })];
    await mount(false, 0, "zzz");
    expect(screen.getByText("No project matches “zzz”.")).toBeTruthy();
    expect(screen.queryByTestId("record-row")).toBeNull();
  });

  it("rename: F2 opens the field, Enter saves the title, Esc cancels", async () => {
    m.items = [item({ dir: "/p/a", title: "Demo" })];
    m.rename.mockClear();
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "F2" }); });
    const field = screen.getByLabelText("Rename") as HTMLInputElement;
    expect(field.value).toBe("Demo");
    // Keys typed into the field never reach the list (Backspace would arm a delete).
    m.del.mockClear();
    await act(async () => { fireEvent.keyDown(field, { key: "Backspace" }); });
    await act(async () => { fireEvent.keyDown(field, { key: "Backspace" }); });
    expect(m.del).not.toHaveBeenCalled();
    await act(async () => { fireEvent.change(field, { target: { value: "Neu" } }); });
    await act(async () => { fireEvent.keyDown(field, { key: "Enter" }); });
    expect(m.rename).toHaveBeenCalledWith("/p/a", "Neu");
    expect(m.edit, "Enter in the field does not open the editor").not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Rename")).toBeNull();
    // Esc leaves without saving.
    await act(async () => { fireEvent.click(screen.getByText("Rename")); });
    const again = screen.getByLabelText("Rename");
    await act(async () => { fireEvent.change(again, { target: { value: "Weg" } }); });
    await act(async () => { fireEvent.keyDown(again, { key: "Escape" }); });
    expect(m.rename).toHaveBeenCalledTimes(1);
    expect(screen.queryByLabelText("Rename")).toBeNull();
  });

  it("a refused rename keeps the field and says why", async () => {
    m.items = [item({ dir: "/p/a", title: "Demo" })];
    m.rename.mockImplementationOnce(async () => { throw "record.title_empty"; });
    await mount();
    await act(async () => { fireEvent.keyDown(window, { key: "F2" }); });
    const field = screen.getByLabelText("Rename");
    await act(async () => { fireEvent.change(field, { target: { value: "  " } }); });
    await act(async () => { fireEvent.keyDown(field, { key: "Enter" }); });
    expect(screen.getByText("The name must not be empty.")).toBeTruthy();
    expect(screen.getByLabelText("Rename")).toBeTruthy();
  });

  it("shows the thumbnail, else a placeholder", async () => {
    m.items = [item({ dir: "/p/a", thumb: "data:image/jpeg;base64,/9j/AA" }), item({ dir: "/p/b", thumb: null })];
    await mount();
    expect((screen.getByTestId("project-thumb") as HTMLImageElement).src).toContain("data:image/jpeg");
    expect(screen.getAllByTestId("project-thumb-empty")).toHaveLength(1);
  });

  it("Open project… opens the picked folder in the editor; a cancelled picker does nothing", async () => {
    m.items = [];
    m.pick.mockResolvedValueOnce(null);
    await mount(false);
    await act(async () => { fireEvent.click(screen.getByText("Open project…")); });
    expect(m.edit).not.toHaveBeenCalled();
    m.pick.mockResolvedValueOnce("/elsewhere/Projekt");
    await act(async () => { fireEvent.click(screen.getByText("Open project…")); });
    expect(m.edit).toHaveBeenCalledWith("/elsewhere/Projekt");
    m.pick.mockImplementationOnce(async () => { throw "record.editor.not_project"; });
    await act(async () => { fireEvent.click(screen.getByText("Open project…")); });
    expect(screen.getByText("This folder is not a recording project (it has no project.json).")).toBeTruthy();
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
