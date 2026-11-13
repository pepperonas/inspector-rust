// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";

const m = vi.hoisted(() => ({
  exp: vi.fn(async (_o?: unknown) => "/Users/x/Downloads/Demo 2026-11-02 09.05.mp4"),
  cancel: vi.fn(async () => {}),
  toast: vi.fn(async () => {}),
  reveal: vi.fn(async () => {}),
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
  info: vi.fn(async (_o: unknown): Promise<unknown> => ({
    width: 1920, height: 1080, codec: "h264", bitrateBps: null, forcedHevc: false, upscale: null,
    programmeMs: 60_000, durationWarning: null, estimatedBytes: null, hasCaptions: true,
  })),
  cover: vi.fn(async (_o: unknown, _ms: number) => "/Users/x/Downloads/Demo – cover.jpg"),
  presets: vi.fn(async () => [
    { id: "youtube", aspect: "16:9", target: "p1080", fps: 30, quality: "standard", loudness: "-14", maxDurationS: null, softDurationS: null, suffix: "youtube" },
    { id: "shorts", aspect: "9:16", target: "p1080", fps: 30, quality: "high", loudness: "-14", maxDurationS: 180, softDurationS: null, suffix: "shorts" },
  ]),
}));
vi.mock("../../lib/ipc", () => ({
  editorExport: m.exp,
  editorCancelExport: m.cancel,
  editorExportInfo: m.info,
  editorExportCover: m.cover,
  editorPlatformPresets: m.presets,
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

async function mount(props: Partial<Parameters<typeof ExportDialog>[0]> = {}) {
  const r = render(<ExportDialog onClose={() => {}} {...props} />);
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
    m.info.mockClear();
    m.cover.mockClear();
    localStorage.clear();
  });

  it("MP4 offers the codec and the sizes up to 4K; GIF hides the codec", async () => {
    await mount();
    expect(screen.getByText("H.264")).toBeTruthy();
    expect(screen.getByText("HEVC")).toBeTruthy();
    for (const size of ["720p", "1080p", "1440p", "4K"]) expect(screen.getByText(size)).toBeTruthy();
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

  it("a platform sets its values and asks for its format once", async () => {
    const onAspect = vi.fn();
    await mount({ aspect: "auto", onAspect });
    await act(async () => { fireEvent.click(screen.getByText("Shorts")); });
    expect(onAspect).toHaveBeenCalledWith("9:16");
    expect(screen.getByText(/Format switched to 9:16/)).toBeTruthy();
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledWith(
      expect.objectContaining({ preset: "shorts", quality: "high", loudness: "-14", fps: 30, target: "p1080" }),
    );
  });

  it("a platform whose format the document already has changes nothing there", async () => {
    const onAspect = vi.fn();
    await mount({ aspect: "16:9", onAspect });
    await act(async () => { fireEvent.click(screen.getByText("YouTube")); });
    expect(onAspect).not.toHaveBeenCalled();
    expect(screen.queryByText(/Format switched/)).toBeNull();
  });

  it("a custom quality sends its number", async () => {
    await mount();
    await act(async () => { fireEvent.click(screen.getAllByText("Custom")[1]); });
    const field = screen.getByLabelText("Bitrate in Mbit/s") as HTMLInputElement;
    await act(async () => { fireEvent.change(field, { target: { value: "500" } }); });
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledWith(expect.objectContaining({ quality: "custom", bitrateMbps: 80 }));
  });

  it("shows what Rust says will come out, warnings included", async () => {
    m.info.mockResolvedValueOnce({
      width: 1080, height: 1920, codec: "hevc", bitrateBps: 12_400_000, forcedHevc: true, upscale: 1.8,
      programmeMs: 200_000, durationWarning: ["toolong", 180], estimatedBytes: 316_000_000, hasCaptions: false,
    });
    localStorage.setItem("ir.record.export.v1", JSON.stringify({ preset: "shorts", quality: "high" }));
    await mount();
    const info = screen.getByTestId("export-info").textContent ?? "";
    expect(info).toContain("1080 × 1920 · HEVC · 12 Mbit/s · about 316 MB");
    expect(info).toContain("exported as HEVC");
    expect(info).toContain("Enlarged 1.8×");
    expect(info).toContain("Longer than Shorts accepts (3:00)");
    // No transcript: the caption file cannot be chosen, and the dialog says why.
    expect((screen.getByText("SRT") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("needs a transcript")).toBeTruthy();
  });

  it("remembers the last choice", async () => {
    const first = await mount();
    await act(async () => { fireEvent.click(screen.getByText("4K")); });
    await act(async () => { fireEvent.click(screen.getByText("-14 LUFS")); });
    first.unmount();
    await mount();
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledWith(expect.objectContaining({ target: "p2160", loudness: "-14" }));
  });

  it("the cover image is taken at the playhead", async () => {
    await mount({ positionMs: 4321.4 });
    await act(async () => { fireEvent.click(screen.getByText("Save cover image")); });
    expect(m.cover).toHaveBeenCalledWith(expect.objectContaining({ kind: "mp4" }), 4321);
    expect(screen.getByText(/Cover image saved to .*cover\.jpg/)).toBeTruthy();
    m.cover.mockRejectedValueOnce("record.editor.export_busy");
    await act(async () => { fireEvent.click(screen.getByText("Save cover image")); });
    expect(screen.getByText("An export is already running.")).toBeTruthy();
  });

  it("offers the project's formats only when there are several", async () => {
    await mount({ aspect: "16:9", formats: ["16:9"] });
    expect(screen.queryByTestId("export-formats")).toBeNull();
  });

  it("exports the ticked formats one after the other, each as its own file", async () => {
    await mount({ aspect: "16:9", formats: ["16:9", "9:16"] });
    const row = screen.getByTestId("export-formats");
    expect(row.querySelector('[aria-pressed="true"]')?.textContent).toBe("16:9");
    await act(async () => { fireEvent.click(screen.getByText("9:16")); });
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledTimes(1);
    expect(m.exp.mock.calls[0][0]).toMatchObject({ variant: "16:9", quiet: true });
    expect(screen.getByTestId("export-batch-pos").textContent).toContain("16:9 · 1 of 2");
    await fire("record-export-done", { path: "/d/a 16x9.mp4" });
    expect(m.exp).toHaveBeenCalledTimes(2);
    expect(m.exp.mock.calls[1][0]).toMatchObject({ variant: "9:16", quiet: false });
    // One failed format is reported next to the one that worked.
    await fire("record-export-failed", { reason: "boom", cancelled: false });
    const done = screen.getByTestId("export-batch-done").textContent ?? "";
    expect(done).toContain("1 of 2 exported");
    expect(done).toContain("/d/a 16x9.mp4");
    expect(done).toContain("9:16: failed");
  });

  it("exports only the active format without a format name when nothing else is ticked", async () => {
    await mount({ aspect: "16:9", formats: ["16:9", "9:16"] });
    await act(async () => { fireEvent.click(lastExport()); });
    expect(m.exp).toHaveBeenCalledTimes(1);
    expect((m.exp.mock.calls[0][0] as { variant?: string } | undefined)?.variant).toBeUndefined();
  });
});
