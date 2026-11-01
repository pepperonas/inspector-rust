// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";

const m = vi.hoisted(() => ({
  exp: vi.fn(async () => "/Users/x/Downloads/Demo 2026-11-02 09.05.mp4"),
  cancel: vi.fn(async () => {}),
  toast: vi.fn(async () => {}),
  reveal: vi.fn(async () => {}),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("../../lib/ipc", () => ({
  editorExport: m.exp,
  editorCancelExport: m.cancel,
  showStatusToast: m.toast,
  revealPath: m.reveal,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    m.handlers.set(name, cb);
    return () => m.handlers.delete(name);
  },
}));
import { ExportDialog } from "./ExportDialog";

const lastExport = () => { const all = screen.getAllByText("Export"); return all[all.length - 1]; };

async function mount() {
  const r = render(<ExportDialog onClose={() => {}} />);
  await act(async () => {});
  return r;
}
const fire = async (name: string, payload: unknown) => {
  await act(async () => { m.handlers.get(name)?.({ payload }); });
};

describe("ExportDialog", () => {
  beforeEach(() => {
    m.exp.mockClear();
    m.cancel.mockClear();
    m.toast.mockClear();
    m.reveal.mockClear();
    m.handlers.clear();
  });

  it("MP4 offers the codec and the three sizes; GIF hides the codec", async () => {
    await mount();
    expect(screen.getByText("H.264")).toBeTruthy();
    expect(screen.getByText("HEVC")).toBeTruthy();
    expect(screen.getByText("720p")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByText("GIF")); });
    expect(screen.queryByText("H.264")).toBeNull();
    expect(screen.queryByText("HEVC")).toBeNull();
    expect(screen.queryByText("720p")).toBeNull();
    for (const w of ["480", "640", "854", "1080"]) expect(screen.getByText(w)).toBeTruthy();
  });

  it("GIF frame rates are 15, 20, 25 and 30", async () => {
    await mount();
    await act(async () => { fireEvent.click(screen.getByText("GIF")); });
    for (const f of ["15", "20", "25", "30"]) expect(screen.getByText(f)).toBeTruthy();
    expect(screen.queryByText("24")).toBeNull();
    expect(screen.queryByText("60")).toBeNull();
  });

  it("sends the chosen options", async () => {
    await mount();
    await act(async () => { fireEvent.click(screen.getByText("HEVC")); });
    await act(async () => { fireEvent.click(screen.getByText("60")); });
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledWith(expect.objectContaining({ kind: "mp4", codec: "hevc", fps: 60, target: "p1080" }));
  });

  it("Cancel during an export calls editorCancelExport", async () => {
    await mount();
    await act(async () => { fireEvent.click(lastExport()); });
    await fire("record-export-progress", { frames: 30, total: 120 });
    expect(screen.getByText("25 %")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByText("Cancel")); });
    expect(m.cancel).toHaveBeenCalledTimes(1);
  });

  it("the progress bar scales, it never animates width", async () => {
    const r = await mount();
    await act(async () => { fireEvent.click(lastExport()); });
    await fire("record-export-progress", { frames: 60, total: 120 });
    const bar = r.container.querySelector("[data-testid=export-progress]") as HTMLElement;
    expect(bar.style.transform).toBe("scaleX(0.5)");
    expect(bar.style.width).toBe("");
  });

  it("success shows where it went; toast + Finder come from Rust, never twice", async () => {
    await mount();
    await act(async () => { fireEvent.click(lastExport()); });
    await fire("record-export-done", { path: "/Users/x/Downloads/Demo 2026-11-02 09.05.mp4" });
    expect(screen.getByText(/Demo 2026-11-02 09\.05\.mp4/)).toBeTruthy();
    expect(m.toast).not.toHaveBeenCalled();
    expect(m.reveal).not.toHaveBeenCalled();
  });
});
