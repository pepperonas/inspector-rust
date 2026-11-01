import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import type { EditorInit, RecordEditDoc, RecordEditorView } from "../lib/ipc";

const editorCurrent = vi.fn();
const editorUpdate = vi.fn();
const editorSeek = vi.fn();
const editorToSource = vi.fn();
const editorPlay = vi.fn();
const editorPosition = vi.fn();
const editorFrame = vi.fn();
const editorResize = vi.fn();
const editorThumbs = vi.fn();
const editorRestart = vi.fn();
const listeners = new Map<string, (e: { payload: unknown }) => void>();

vi.mock("../lib/ipc", () => ({
  editorCurrent: () => editorCurrent(),
  editorUpdate: (id: number, d: RecordEditDoc) => editorUpdate(d, id),
  // Like serde on the Rust side: a fractional u64 is an error, not a value.
  editorSeek: (t: number) => (Number.isInteger(t) ? editorSeek(t) : Promise.reject(new Error(`float ${t}`))),
  editorToSource: (t: number) =>
    Number.isInteger(t) ? editorToSource(t) : Promise.reject(new Error(`float ${t}`)),
  editorPlay: (p: boolean) => editorPlay(p),
  editorPosition: () => editorPosition(),
  editorFrame: (g: number) => editorFrame(g),
  editorResize: (w: number, h: number) => editorResize(w, h),
  editorThumbs: (n: number) => editorThumbs(n),
  editorRestart: () => editorRestart(),
  editorExport: vi.fn(),
  editorCancelExport: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return () => listeners.delete(name);
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}` }));

import { RecordEditor } from "./RecordEditor";

const doc0: RecordEditDoc = {
  version: 1,
  clips: [{ startMs: 0, endMs: 10_000 }],
  speed: [],
  crop: [null],
  output: { aspect: "source" },
  micGainDb: 0,
};
const viewOf = (doc: RecordEditDoc): RecordEditorView => {
  let t = 0;
  const clips = doc.clips.map((c) => {
    const s = t;
    t += c.endMs - c.startMs;
    return { progStartMs: s, progEndMs: t };
  });
  // Spans for the mock only (1× everywhere): the component never computes them itself.
  const speed = doc.speed.map((r, index) => ({ index, progStartMs: r.startMs, progEndMs: r.endMs }));
  return { doc, programmeMs: t, clips, speed };
};
const init = (helperError: string | null = null, sessionId = 1, title = "Take", view = viewOf(doc0)): EditorInit => ({
  sessionId,
  project: { dir: "/m/take", title, createdAt: 0, durationMs: 10_000, status: "ready", failure: null },
  meta: {
    source: "display", sourceLabel: "D", width: 1920, height: 1080, fps: 30, durationMs: 10_000,
    systemAudio: true, mic: { offsetMs: 0, sampleRate: 48_000 }, cursorMode: "hidden" as never, pauses: [],
  },
  view,
  helperError,
});
const playback = (playing: boolean, positionMs = 0) => ({
  playing, positionMs, programmeMs: 10_000, exporting: false, helperError: null,
  source: { clip: 0, sourceMs: positionMs }, speed: 1,
});

const lastSent = () => editorUpdate.mock.calls[editorUpdate.mock.calls.length - 1]?.[0];

async function mount() {
  const r = render(<RecordEditor />);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(20);
  });
  return r;
}

beforeEach(() => {
  vi.useFakeTimers();
  editorCurrent.mockResolvedValue(init());
  editorUpdate.mockImplementation(async (d: RecordEditDoc) => viewOf(d));
  editorSeek.mockResolvedValue({ clip: 0, sourceMs: 4000 });
  editorToSource.mockResolvedValue({ clip: 0, sourceMs: 4000 });
  editorPlay.mockImplementation(async (p: boolean) => playback(p));
  editorPosition.mockResolvedValue(playback(false, 4000));
  editorFrame.mockResolvedValue(null);
  editorResize.mockResolvedValue(undefined);
  editorThumbs.mockResolvedValue([]);
  editorRestart.mockResolvedValue(playback(false));
});
afterEach(() => {
  cleanup();
  listeners.clear();
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("RecordEditor", () => {
  it("opening another project into the open window reloads the view and restarts frame tracking", async () => {
    editorCurrent.mockResolvedValue(init(null, 1, "Take A"));
    editorFrame.mockResolvedValueOnce({ gen: 5, width: 2, height: 2, rgba: new Uint8Array(16) });
    const { getByText, getAllByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(editorFrame.mock.calls.some((c) => c[0] === 5), "A's frame generation is tracked").toBe(true);
    expect(getByText("Take A")).toBeTruthy();

    // B is opened (a new Rust session; its helper scene counts frames from 0 again).
    const docB: RecordEditDoc = { ...doc0, clips: [{ startMs: 0, endMs: 3000 }, { startMs: 5000, endMs: 9000 }], crop: [null, null] };
    editorCurrent.mockResolvedValue(init(null, 2, "Take B", viewOf(docB)));
    await act(async () => {
      listeners.get("record-editor-opened")?.({ payload: { sessionId: 2 } });
    });
    const before = editorFrame.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(getByText("Take B")).toBeTruthy();
    expect(getAllByTestId("ed-clip")).toHaveLength(2);
    const after = editorFrame.mock.calls.slice(before);
    expect(after.length).toBeGreaterThan(0);
    expect(after.every((c) => c[0] === 0), "frame tracking restarts at 0").toBe(true);

    // Edits now go to B's session, with B's doc.
    editorToSource.mockResolvedValue({ clip: 1, sourceMs: 6000 });
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    const [sent, id] = editorUpdate.mock.calls[editorUpdate.mock.calls.length - 1];
    expect(id).toBe(2);
    expect((sent as RecordEditDoc).clips.length).toBe(3);

    // The same session announced again changes nothing.
    const reads = editorCurrent.mock.calls.length;
    await act(async () => {
      listeners.get("record-editor-opened")?.({ payload: { sessionId: 2 } });
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(editorCurrent.mock.calls.length).toBe(reads);
  });

  it("Space starts playback", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: " ", code: "Space" });
    });
    expect(editorPlay).toHaveBeenCalledWith(true);
  });

  it("S splits at the playhead's source position and sends the doc to Rust", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorToSource).toHaveBeenCalled();
    expect(editorSeek).not.toHaveBeenCalled(); // asking where the playhead is never moves it
    const sent = lastSent() as RecordEditDoc;
    expect(sent.clips).toEqual([
      { startMs: 0, endMs: 4000 },
      { startMs: 4000, endMs: 10_000 },
    ]);
  });

  it("⌘Z restores the previous doc through editorUpdate", async () => {
    const { getAllByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(getAllByTestId("ed-clip")).toHaveLength(2);
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).clips).toEqual(doc0.clips);
    expect(getAllByTestId("ed-clip")).toHaveLength(1);
  });

  it("keys typed into a field do not drive the editor", async () => {
    const { container } = await mount();
    const input = document.createElement("input");
    container.appendChild(input);
    await act(async () => {
      fireEvent.keyDown(input, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
  });

  it("the fatal event greys the preview and offers a restart", async () => {
    const { getByText, getByTestId } = await mount();
    await act(async () => {
      listeners.get("record-editor-fatal")?.({ payload: { reason: "helper crashed" } });
    });
    expect(getByTestId("ed-fatal").textContent).toContain("helper crashed");
    await act(async () => {
      fireEvent.click(getByText("Restart"));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorRestart).toHaveBeenCalledTimes(1);
  });

  it("a helper error at open shows the same restart state", async () => {
    editorCurrent.mockResolvedValue(init("record.editor.helper_missing"));
    const { getByTestId } = await mount();
    expect(getByTestId("ed-fatal").textContent).toContain("render helper");
  });

  it("ArrowRight at 30 fps seeks to an integer ms (a fraction would fail in Rust)", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowRight" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorSeek).toHaveBeenCalledWith(33);
    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowRight" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorSeek).toHaveBeenLastCalledWith(66); // 33 + 33.33, rounded
    for (const [v] of editorSeek.mock.calls) expect(Number.isInteger(v)).toBe(true);
  });

  it("a failed IPC is shown, not swallowed", async () => {
    editorSeek.mockRejectedValueOnce("record.editor.no_session");
    const { getByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowRight", shiftKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(getByTestId("ed-notice").textContent).toContain("No recording is open");
  });

  // Edits must start from History's present, not from a `view` that waits for Rust (I3).
  const withRegion: RecordEditDoc = { ...doc0, speed: [{ clip: 0, startMs: 5000, endMs: 6000, speed: 2 }] };
  const pickSpeed = async (r: ReturnType<typeof render>, label: string) => {
    const band = r.getByTestId("ed-speed");
    await act(async () => {
      fireEvent.pointerDown(band, { clientY: 100 });
    });
    await act(async () => {
      fireEvent.pointerUp(band, { clientY: 100 });
    });
    await act(async () => {
      fireEvent.click(r.getByText(label));
      await vi.advanceTimersByTimeAsync(1);
    });
  };

  it("two quick edits: the second builds on the first while Rust still answers", async () => {
    editorCurrent.mockResolvedValue({ ...init(), view: viewOf(withRegion) });
    const r = await mount();
    let release: () => void = () => undefined;
    editorUpdate.mockImplementationOnce(
      (d: RecordEditDoc) => new Promise((res) => (release = () => res(viewOf(d)))),
    );
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" }); // split at 4000 — answer held back
      await vi.advanceTimersByTimeAsync(1);
    });
    await pickSpeed(r, "4×");
    await act(async () => {
      release();
      await vi.advanceTimersByTimeAsync(5);
    });
    const sent = lastSent() as RecordEditDoc;
    expect(sent.clips).toHaveLength(2);
    expect(sent.speed).toEqual([{ clip: 1, startMs: 5000, endMs: 6000, speed: 4 }]);
    expect(r.getAllByTestId("ed-clip")).toHaveLength(2);
  });

  it("an edit right after undo builds on the undone document", async () => {
    editorCurrent.mockResolvedValue({ ...init(), view: viewOf(withRegion) });
    const r = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    let release: () => void = () => undefined;
    editorUpdate.mockImplementationOnce(
      (d: RecordEditDoc) => new Promise((res) => (release = () => res(viewOf(d)))),
    );
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true }); // undo — answer held back
      await vi.advanceTimersByTimeAsync(1);
    });
    await pickSpeed(r, "8×");
    await act(async () => {
      release();
      await vi.advanceTimersByTimeAsync(5);
    });
    const sent = lastSent() as RecordEditDoc;
    expect(sent.clips).toEqual(doc0.clips);
    expect(sent.speed).toEqual([{ clip: 0, startMs: 5000, endMs: 6000, speed: 8 }]);
  });

  it("a failed update resyncs the timeline from Rust", async () => {
    const r = await mount();
    const rustDoc: RecordEditDoc = { ...doc0, clips: [{ startMs: 0, endMs: 4000 }, { startMs: 4000, endMs: 10_000 }], crop: [null, null] };
    editorUpdate.mockRejectedValueOnce("render helper stalled");
    editorCurrent.mockResolvedValue({ ...init("render helper stalled"), view: viewOf(rustDoc) });
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorCurrent).toHaveBeenCalledTimes(2);
    expect(r.getAllByTestId("ed-clip")).toHaveLength(2);
    expect(r.getByTestId("ed-fatal").textContent).toContain("render helper stalled");
  });

  it("Restart resyncs the document and timeline", async () => {
    const r = await mount();
    await act(async () => {
      listeners.get("record-editor-fatal")?.({ payload: { reason: "boom" } });
    });
    const rustDoc: RecordEditDoc = { ...doc0, clips: [{ startMs: 0, endMs: 3000 }, { startMs: 5000, endMs: 9000 }], crop: [null, null] };
    editorCurrent.mockResolvedValue({ ...init(), view: viewOf(rustDoc) });
    await act(async () => {
      fireEvent.click(r.getByText("Restart"));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorCurrent).toHaveBeenCalledTimes(2);
    expect(r.getAllByTestId("ed-clip")).toHaveLength(2);
    expect(r.queryByTestId("ed-fatal")).toBeNull();
  });

  it("held keys do not repeat edits", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s", repeat: true });
      fireEvent.keyDown(window, { key: " ", repeat: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
    expect(editorPlay).not.toHaveBeenCalled();
  });
});
