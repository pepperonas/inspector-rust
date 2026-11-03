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
const editorAssets = vi.fn();
const editorImportAsset = vi.fn();
const editorDuplicateClip = vi.fn();
const editorMoveClip = vi.fn();
const editorImportTake = vi.fn();
const editorVoiceoverStart = vi.fn();
const editorVoiceoverStop = vi.fn();
const editorVoiceoverCancel = vi.fn();
const editorTrackEdge = vi.fn(async (..._a: unknown[]): Promise<unknown> => null);
const recordPresetApply = vi.fn();
const recordPresetSave = vi.fn();
const openDialog = vi.fn();
const editorOpen = vi.fn(async (_dir: string, _o?: unknown): Promise<unknown> => ({}));
const projects = vi.fn(async (): Promise<unknown[]> => []);
const getRecordShortcuts = vi.fn(async (): Promise<unknown> => ({}));
const setRecordShortcuts = vi.fn(async (s: unknown) => s);
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
  editorAssets: () => editorAssets(),
  editorImportAsset: (id: number, src: string, kind: string) => editorImportAsset(id, src, kind),
  editorDuplicateClip: (id: number, i: number) => editorDuplicateClip(id, i),
  editorMoveClip: (id: number, f: number, t: number) => editorMoveClip(id, f, t),
  editorImportTake: (id: number, d: string) => editorImportTake(id, d),
  editorVoiceoverStart: (id: number, ms: number) => editorVoiceoverStart(id, ms),
  editorVoiceoverStop: (id: number) => editorVoiceoverStop(id),
  editorVoiceoverCancel: (id: number) => editorVoiceoverCancel(id),
  editorTrackEdge: (...a: unknown[]) => editorTrackEdge(...a),
  EV_VOICEOVER_LEVEL: "record-voiceover-level",
  EV_VOICEOVER_FINISHED: "record-voiceover-finished",
  recordListProjects: () => projects(),
  recordOpenDialog: async () => null,
  editorOpen: (d: string, o?: unknown) => editorOpen(d, o),
  recordPresetsList: async () => ({ presets: [{ name: "Dark", file: "Dark.irpreset", ok: true, error: null }], defaultFile: null }),
  recordPresetApply: (id: number, file: string) => recordPresetApply(id, file),
  recordPresetSave: (id: number, name: string) => recordPresetSave(id, name),
  editorExport: vi.fn(),
  editorCancelExport: vi.fn(),
  EV_RECORD_SHORTCUTS: "record-shortcuts-changed",
  getRecordShortcuts: () => getRecordShortcuts(),
  setRecordShortcuts: (s: unknown) => setRecordShortcuts(s),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return () => listeners.delete(name);
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}` }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (o: unknown) => openDialog(o) }));

import { RecordEditor } from "./RecordEditor";

const doc0: RecordEditDoc = {
  version: 3,
  clips: [{ startMs: 0, endMs: 10_000 }],
  speed: [],
  crop: [null],
  output: { aspect: "source" },
  micGainDb: 0,
  appearance: {
    background: { kind: "color", color: "#000000" },
    backgroundMotion: "none",
    padding: 0,
    radiusPx: 0,
    shadow: 0,
    backgroundBlur: 0,
    motionBlur: 0,
    frame: "none",
    frameTheme: "light",
  },
  cursor: {
    show: false,
    autoHide: false,
    theme: "default",
    custom: null,
    size: 3.6,
    smoothing: 0.3,
    motionBlur: 0.35,
    clickBounce: 1,
    clickImpact: false,
    clickSound: false,
    clickSoundGainDb: 0,
  },
  zooms: [],
  autoZoomDone: true,
  annotations: [],
  audioTracks: [],
  systemGainDb: 0,
  camera: {
    preset: "no-webcam",
    sizePct: 40,
    mask: "square",
    roundness: null,
    mirror: false,
    reactiveZoom: true,
    anchor: "bottom-right",
    crop: null,
    background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
  },
  layoutRegions: [],
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
  const zooms = doc.zooms.map((z, index) => ({ index, progStartMs: z.startMs, progEndMs: z.endMs }));
  const annotationSpans = doc.annotations.map((a, index) => ({ index, progStartMs: a.startMs, progEndMs: a.endMs }));
  const tracks = doc.audioTracks.map((a, index) => {
    const len = a.spanMs ?? a.durationMs - a.offsetMs;
    return {
      index, repeat: 0, progStartMs: a.startMs, progEndMs: a.startMs + len, fileStartMs: a.offsetMs, fileEndMs: a.offsetMs + len,
      fadeInMs: a.fadeInMs, fadeOutMs: a.fadeOutMs, gainDb: a.gainDb, kind: a.kind, muted: a.muted,
    };
  });
  return { doc, programmeMs: t, clips, speed, zooms, annotationSpans, tracks, clickHits: [], screenRects: doc.clips.map(() => [0, 0, 1, 1]), outputSize: [1920, 1080] };
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
  localStorage.clear();
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
  editorAssets.mockResolvedValue({ wallpapers: [], themes: [], hasCursorData: true, clickDown: null, clickUp: null });
  editorImportAsset.mockResolvedValue({ file: "img.png" });
  openDialog.mockResolvedValue(null);
  editorVoiceoverStart.mockResolvedValue({ sessionId: 1, file: "voiceover-1.wav", programmeMs: 4000, maxMs: 6000, clip: 0, sourceMs: 4000 });
  editorVoiceoverCancel.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  listeners.clear();
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("RecordEditor", () => {
  it("a failing asset listing is shown as a notice", async () => {
    editorCurrent.mockResolvedValue(init(null, 1, "Take A"));
    editorAssets.mockRejectedValue("no assets");
    const { getByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(getByTestId("ed-notice").getAttribute("title")).toBe("no assets");
  });

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

  it("a rename of the open project shows in the header at once; another session's is ignored", async () => {
    const { getByText, queryByText } = await mount();
    await act(async () => {
      listeners.get("record-editor-retitled")?.({ payload: { sessionId: 99, title: "Not mine" } });
    });
    expect(queryByText("Not mine")).toBeNull();
    await act(async () => {
      listeners.get("record-editor-retitled")?.({ payload: { sessionId: 1, title: "Renamed take" } });
    });
    expect(getByText("Renamed take")).toBeTruthy();
  });

  it("Space starts playback", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: " ", code: "Space" });
    });
    expect(editorPlay).toHaveBeenCalledWith(true);
  });

  it("keys stand still while the Insert-take popover is open", async () => {
    const { getByTestId } = await mount();
    await act(async () => {
      fireEvent.click(getByTestId("ed-insert-take"));
    });
    expect(getByTestId("ed-insert-take").getAttribute("aria-expanded")).toBe("true");
    const sentBefore = editorUpdate.mock.calls.length;
    await act(async () => {
      fireEvent.keyDown(window, { key: " ", code: "Space" });
      fireEvent.keyDown(window, { key: "s" });
      fireEvent.keyDown(window, { key: "Backspace" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorPlay).not.toHaveBeenCalled();
    expect(editorUpdate.mock.calls.length).toBe(sentBefore);
    // Esc belongs to the popover: it closes, and the keys work again.
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(getByTestId("ed-insert-take").getAttribute("aria-expanded")).toBe("false");
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

  it("the preview box and its render size follow Rust's output size, not the source shape", async () => {
    // Auto with padding: Rust answers a square output for a 16:9 source.
    const square = { ...viewOf({ ...doc0, output: { aspect: "auto" } }), outputSize: [1080, 1080] as [number, number] };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", square));
    const observed: ((e: { contentRect: { width: number; height: number } }[]) => void)[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(cb: (e: { contentRect: { width: number; height: number } }[]) => void) {
          observed.push(cb);
        }
        observe() {
          observed.forEach((cb) => cb([{ contentRect: { width: 800, height: 500 } }]));
        }
        disconnect() {}
      },
    );
    try {
      const { getByTestId } = await mount();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(300);
      });
      const box = getByTestId("ed-canvas").parentElement as HTMLElement;
      expect(box.style.width).toBe("500px");
      expect(box.style.height).toBe("500px");
      const [w, h] = editorResize.mock.calls[editorResize.mock.calls.length - 1];
      expect(w).toBe(h);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("an inspector slider previews live and is one undo step on release", async () => {
    const { getByRole } = await mount();
    await act(async () => {
      fireEvent.click(getByRole("tab", { name: "Frame" }));
    });
    const pad = getByRole("slider", { name: "Padding" }) as HTMLInputElement;
    await act(async () => {
      fireEvent.change(pad, { target: { value: "10" } });
      fireEvent.change(pad, { target: { value: "20" } });
      fireEvent.change(pad, { target: { value: "30" } });
      await vi.advanceTimersByTimeAsync(20);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
    expect(pad.value).toBe("30");
    // Rust answers the live send late, after the finger moved on: the slider must not jump back.
    let answer: (v: RecordEditorView) => void = () => {};
    editorUpdate.mockImplementationOnce((d: RecordEditDoc) => new Promise((r) => (answer = () => r(viewOf(d)))));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(editorUpdate).toHaveBeenCalledTimes(1);
    expect((lastSent() as RecordEditDoc).appearance.padding).toBe(30);
    await act(async () => {
      fireEvent.change(pad, { target: { value: "35" } });
      answer(viewOf(doc0));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(pad.value).toBe("35");
    await act(async () => {
      fireEvent.change(pad, { target: { value: "40" } });
      fireEvent.pointerUp(pad);
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).toHaveBeenCalledTimes(2);
    expect((lastSent() as RecordEditDoc).appearance.padding).toBe(40);
    // One ⌘Z goes back to before the drag.
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).appearance.padding).toBe(0);
  });

  it("an own image is picked, copied into the project and set as the background", async () => {
    openDialog.mockResolvedValue("/Users/x/Pictures/beach.png");
    const { getByRole } = await mount();
    await act(async () => {
      fireEvent.click(getByRole("button", { name: "Choose image…" }));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorImportAsset).toHaveBeenCalledWith(1, "/Users/x/Pictures/beach.png", "image");
    expect((lastSent() as RecordEditDoc).appearance.background).toEqual({ kind: "image", file: "img.png" });
  });

  it("the settings column folds away", async () => {
    const { getByTestId, queryByTestId } = await mount();
    expect(queryByTestId("ed-inspector")).toBeTruthy();
    await act(async () => {
      fireEvent.click(getByTestId("ed-inspector-toggle"));
    });
    expect(queryByTestId("ed-inspector")).toBeNull();
  });

  it("⌘D duplicates the clip through Rust and is one undo step", async () => {
    const twice: RecordEditDoc = { ...doc0, clips: [doc0.clips[0], doc0.clips[0]], crop: [null, null] };
    editorDuplicateClip.mockResolvedValue(viewOf(twice));
    const { getAllByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "d", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorDuplicateClip).toHaveBeenCalledWith(1, 0);
    expect(getAllByTestId("ed-clip")).toHaveLength(2);
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).clips).toEqual(doc0.clips);
  });

  it("applying a style preset goes through Rust and is one undo step", async () => {
    const styled: RecordEditDoc = {
      ...doc0,
      output: { aspect: "9:16" },
      appearance: { ...doc0.appearance, padding: 12, frame: "laptop" },
    };
    recordPresetApply.mockResolvedValue(viewOf(styled));
    const { getByTestId, getByText, queryByTestId } = await mount();
    fireEvent.click(getByTestId("ed-presets-open"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.click(getByText("Dark"));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(recordPresetApply).toHaveBeenCalledTimes(1);
    expect(recordPresetApply).toHaveBeenCalledWith(1, "Dark.irpreset");
    expect(queryByTestId("ed-presets")).toBeNull();
    expect(getByTestId("ed-info").textContent).toBe("Style “Dark” applied");
    // One step back is the look before the preset, in one go.
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(lastSent() as RecordEditDoc).toEqual(doc0);
    // …and forward again is the preset's look.
    await act(async () => {
      fireEvent.keyDown(window, { key: "z", metaKey: true, shiftKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(lastSent() as RecordEditDoc).toEqual(styled);
  });

  it("while the presets menu is open, editor keys do nothing; Esc closes it", async () => {
    const { getByTestId, queryByTestId } = await mount();
    fireEvent.click(getByTestId("ed-presets-open"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      fireEvent.keyDown(window, { key: " " });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
    expect(editorPlay).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(queryByTestId("ed-presets")).toBeNull();
  });

  it("saving a preset waits for a pending edit to reach Rust", async () => {
    const order: string[] = [];
    let release: () => void = () => undefined;
    editorUpdate.mockImplementation(
      (d: RecordEditDoc) =>
        new Promise((res) => {
          release = () => {
            order.push("update");
            res(viewOf(d));
          };
        }),
    );
    recordPresetSave.mockImplementation(async () => {
      order.push("save");
      return { name: "Mine", file: "Mine.irpreset", ok: true, error: null };
    });
    const { getByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    fireEvent.click(getByTestId("ed-presets-open"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    fireEvent.click(getByTestId("ed-preset-save-open"));
    fireEvent.change(getByTestId("ed-preset-name"), { target: { value: "Mine" } });
    await act(async () => {
      fireEvent.click(getByTestId("ed-preset-save"));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(recordPresetSave).not.toHaveBeenCalled();
    await act(async () => {
      release();
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(order).toEqual(["update", "save"]);
    expect(recordPresetSave).toHaveBeenCalledWith(1, "Mine");
  });

  it("the audio preview plays the inserted take's files in its clip", async () => {
    const withTake: RecordEditDoc = {
      ...doc0,
      clips: [doc0.clips[0], { startMs: 0, endMs: 5_000, source: "t1" }],
      crop: [null, null],
    };
    const view = {
      ...viewOf(withTake),
      takes: { t1: { durationMs: 5_000, width: 1280, height: 720, systemAudio: true, hasMic: false, micOffsetMs: 0, hasCursor: false } },
    };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", view));
    editorSeek.mockResolvedValue({ clip: 1, sourceMs: 1_000 });
    const { getByTestId, queryByTestId, getAllByTestId } = await mount();
    expect(getByTestId("ed-audio-system").getAttribute("src")).toBe("asset:///m/take/screen.mp4");
    await act(async () => {
      fireEvent.pointerDown(getAllByTestId("ed-track")[0], { clientX: 0, pointerId: 1 });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(getByTestId("ed-audio-system").getAttribute("src")).toBe("asset:///m/take/takes/t1/screen.mp4");
    expect(queryByTestId("ed-audio-mic")).toBeNull();
  });

  it("crop on an inserted take's clip uses that take's video and size, presets included", async () => {
    const withTake: RecordEditDoc = {
      ...doc0,
      clips: [doc0.clips[0], { startMs: 0, endMs: 5_000, source: "t1" }],
      crop: [null, null],
    };
    const takes = { t1: { durationMs: 5_000, width: 1080, height: 1920, systemAudio: false, hasMic: false, micOffsetMs: 0, hasCursor: false } };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", { ...viewOf(withTake), takes }));
    editorUpdate.mockImplementation(async (d: RecordEditDoc) => ({ ...viewOf(d), takes }));
    const { getAllByTestId, getByTestId, getByText } = await mount();
    await act(async () => {
      fireEvent.pointerDown(getAllByTestId("ed-clip")[1], { clientX: 0, pointerId: 1 });
      fireEvent.pointerUp(getAllByTestId("ed-clip")[1], { clientX: 0, pointerId: 1 });
      fireEvent.keyDown(window, { key: "c" });
      await vi.advanceTimersByTimeAsync(5);
    });
    const crop = getByTestId("ed-crop");
    expect(crop.querySelector("video")?.getAttribute("src")).toBe("asset:///m/take/takes/t1/screen.mp4");
    await act(async () => {
      fireEvent.click(getByText("1:1"));
      await vi.advanceTimersByTimeAsync(5);
    });
    // 1:1 on the take's portrait 1080×1920 frame: full width, 1080/1920 of the height.
    const rect = (lastSent() as RecordEditDoc).crop[1]!;
    expect(rect.w).toBeCloseTo(1);
    expect(rect.h).toBeCloseTo(1080 / 1920);
  });

  it("a clip whose take is missing is marked and cannot be cropped", async () => {
    const withGone: RecordEditDoc = {
      ...doc0,
      clips: [doc0.clips[0], { startMs: 0, endMs: 5_000, source: "gone" }],
      crop: [null, null],
    };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", { ...viewOf(withGone), takes: {} }));
    const { getAllByTestId, queryByTestId, getByTestId } = await mount();
    const clips = getAllByTestId("ed-clip");
    expect(clips[1].hasAttribute("data-missing")).toBe(true);
    expect(clips[0].hasAttribute("data-missing")).toBe(false);
    await act(async () => {
      fireEvent.pointerDown(clips[1], { clientX: 0, pointerId: 1 });
      fireEvent.pointerUp(clips[1], { clientX: 0, pointerId: 1 });
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "c" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(queryByTestId("ed-crop")).toBeNull();
    expect(getByTestId("ed-notice").textContent).toMatch(/missing/i);
  });

  it("⌘D duplicates the selected clip, not the one under the playhead", async () => {
    const two: RecordEditDoc = { ...doc0, clips: [{ startMs: 0, endMs: 4_000 }, { startMs: 4_000, endMs: 10_000 }], crop: [null, null] };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", viewOf(two)));
    editorDuplicateClip.mockResolvedValue(viewOf(two));
    const { getAllByTestId } = await mount();
    // The playhead is at 0 (clip 0); clip 1 is selected by a click.
    await act(async () => {
      fireEvent.pointerDown(getAllByTestId("ed-clip")[1], { clientX: 0, pointerId: 1 });
      fireEvent.pointerUp(getAllByTestId("ed-clip")[1], { clientX: 0, pointerId: 1 });
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "d", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorDuplicateClip).toHaveBeenCalledWith(1, 1);
  });

  it("an annotation picked with Tab opens its settings and preview handles; typing in it drives nothing", async () => {
    const note: RecordEditDoc = {
      ...doc0,
      annotations: [
        {
          id: "t1", clip: 0, startMs: 1_000, endMs: 3_000, x: 10, y: 10, w: 30, h: 20, zIndex: 1,
          body: {
            kind: "text", content: "", color: "#ffffff", background: "transparent", fontSize: 32, fontFamily: "Inter",
            bold: true, italic: false, underline: false, align: "center", animation: "none",
          },
        },
      ],
    };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", viewOf(note)));
    const { getByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(document.body, { key: "Tab" });
    });
    expect(getByTestId("ed-ann-overlay")).toBeTruthy();
    const field = getByTestId("ed-ann-content");
    editorToSource.mockClear();
    await act(async () => {
      for (const key of ["s", "S", "c", "a", "z", " ", "Tab", "Backspace", "Delete"]) fireEvent.keyDown(field, { key });
      fireEvent.change(field, { target: { value: "sacz" } });
      await vi.advanceTimersByTimeAsync(5);
    });
    // No split, crop, new annotation or zoom, no delete: the keys were text.
    expect(editorToSource).not.toHaveBeenCalled();
    expect(document.querySelector('[data-testid="ed-crop"]')).toBeNull();
    await act(async () => {
      fireEvent.blur(field);
      await vi.advanceTimersByTimeAsync(400);
    });
    const sent = lastSent() as RecordEditDoc;
    expect(sent.clips).toEqual(note.clips);
    expect(sent.annotations).toHaveLength(1);
    expect(sent.annotations[0].body).toMatchObject({ kind: "text", content: "sacz" });
  });
  it("the annotation overlay is not mounted while a dialog or popover owns the keys", async () => {
    const note: RecordEditDoc = {
      ...doc0,
      annotations: [
        {
          id: "t1", clip: 0, startMs: 0, endMs: 3_000, x: 10, y: 10, w: 30, h: 20, zIndex: 1,
          body: {
            kind: "text", content: "", color: "#ffffff", background: "transparent", fontSize: 32, fontFamily: "Inter",
            bold: true, italic: false, underline: false, align: "center", animation: "none",
          },
        },
      ],
    };
    editorCurrent.mockResolvedValue(init(null, 1, "Take", viewOf(note)));
    const { getByTestId, queryByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(document.body, { key: "Tab" });
    });
    expect(getByTestId("ed-ann-overlay")).toBeTruthy();
    for (const opener of ["ed-presets-open", "ed-insert-take"]) {
      await act(async () => {
        fireEvent.click(getByTestId(opener));
      });
      expect(queryByTestId("ed-ann-overlay"), opener).toBeNull();
      await act(async () => {
        fireEvent.keyDown(window, { key: "Escape" });
      });
      expect(getByTestId("ed-ann-overlay"), `${opener} closed`).toBeTruthy();
    }
  });
  describe("audio", () => {
    const voTrack = {
      id: "vo-1", kind: "voiceover" as const, file: "voiceover-1.wav", clip: 0, startMs: 4000, durationMs: 2000, offsetMs: 0,
      spanMs: 2000, gainDb: 0, looped: false, fadeInMs: 0, fadeOutMs: 0, muted: false, label: "",
    };

    it("M picks an audio file, copies it as audio and lays a music track at the playhead", async () => {
      openDialog.mockResolvedValue("/Users/ana/Music/Calm Piano.m4a");
      editorImportAsset.mockResolvedValue({ file: "f00d.m4a", durationMs: 61_000 });
      const { getByTestId } = await mount();
      await act(async () => {
        fireEvent.keyDown(window, { key: "m" });
        await vi.advanceTimersByTimeAsync(5);
      });
      const opts = openDialog.mock.calls[0][0] as { filters: { extensions: string[] }[] };
      expect(opts.filters[0].extensions).toEqual(["mp3", "m4a", "aac", "wav", "flac"]);
      expect(editorImportAsset).toHaveBeenCalledWith(1, "/Users/ana/Music/Calm Piano.m4a", "audio");
      const t = (lastSent() as RecordEditDoc).audioTracks[0];
      expect(t).toMatchObject({ kind: "music", file: "f00d.m4a", durationMs: 61_000, clip: 0, startMs: 4000, gainDb: -18, label: "Calm Piano" });
      // The new track is selected: the audio tab shows its settings.
      expect(getByTestId("ed-audio-track-settings")).toBeTruthy();
    });

    it("V starts a take at the playhead, V again stops it: start, then stop, one undo step", async () => {
      const withVo = { ...doc0, audioTracks: [voTrack] };
      editorVoiceoverStop.mockResolvedValue(viewOf(withVo));
      const { getByTestId, getAllByTestId, queryByTestId } = await mount();
      await act(async () => {
        fireEvent.keyDown(window, { key: "v" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(editorVoiceoverStart).toHaveBeenCalledWith(1, 0);
      expect(getByTestId("ed-vo-panel")).toBeTruthy();
      // Editing is off while the take runs: the timeline is inert, S does nothing.
      expect(getByTestId("ed-track").closest("[data-locked]")).toBeTruthy();
      await act(async () => {
        listeners.get("record-voiceover-level")?.({ payload: { rms: 0.1, elapsedMs: 1200, ended: false } });
        fireEvent.keyDown(window, { key: "s" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(editorUpdate).not.toHaveBeenCalled();
      for (const m of getAllByTestId("ed-vo-meter")) expect(m.style.transform).not.toBe("scaleX(0)");
      await act(async () => {
        fireEvent.keyDown(window, { key: "v" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(editorVoiceoverStop).toHaveBeenCalledWith(1);
      expect(editorVoiceoverStart.mock.invocationCallOrder[0]).toBeLessThan(editorVoiceoverStop.mock.invocationCallOrder[0]);
      expect(queryByTestId("ed-vo-panel")).toBeNull();
      expect(getByTestId("ed-audio-bar")).toBeTruthy();
      // ⌘Z takes the voiceover back out.
      await act(async () => {
        fireEvent.keyDown(window, { key: "z", metaKey: true });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect((lastSent() as RecordEditDoc).audioTracks).toEqual([]);
    });

    it("a track edge is resolved only after the edits on their way reached Rust", async () => {
      const withVo = { ...doc0, audioTracks: [voTrack] };
      editorCurrent.mockResolvedValue(init(null, 1, "Take", viewOf(withVo)));
      vi.stubGlobal(
        "ResizeObserver",
        class {
          constructor(private cb: (e: { contentRect: { width: number; height: number } }[]) => void) {}
          observe() {
            this.cb([{ contentRect: { width: 800, height: 40 } }]);
          }
          disconnect() {}
        },
      );
      try {
        const { getByTestId } = await mount();
        let release: () => void = () => undefined;
        editorUpdate.mockImplementationOnce(
          (d: RecordEditDoc) => new Promise((r) => (release = () => r(viewOf(d)))),
        );
        editorTrackEdge.mockClear();
        await act(async () => {
          fireEvent.keyDown(window, { key: "s" }); // an edit on its way, not answered yet
          await vi.advanceTimersByTimeAsync(5);
        });
        expect(editorUpdate).toHaveBeenCalled();
        const edge = getByTestId("ed-audio-end");
        // Separate events (each its own act): the drag state must land between them.
        fireEvent.pointerDown(edge, { clientX: 300, pointerId: 1 });
        fireEvent.pointerMove(edge, { clientX: 260, pointerId: 1 });
        fireEvent.pointerUp(edge, { clientX: 260, pointerId: 1 });
        await act(async () => {
          await vi.advanceTimersByTimeAsync(5);
        });
        expect(editorTrackEdge, "waits for the pending edit").not.toHaveBeenCalled();
        await act(async () => {
          release();
          await vi.advanceTimersByTimeAsync(5);
        });
        expect(editorTrackEdge).toHaveBeenCalledTimes(1);
      } finally {
        vi.unstubAllGlobals();
      }
    });

    it("Esc discards a running take", async () => {
      const { queryByTestId } = await mount();
      await act(async () => {
        fireEvent.keyDown(window, { key: "v" });
        await vi.advanceTimersByTimeAsync(5);
      });
      await act(async () => {
        fireEvent.keyDown(window, { key: "Escape" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(editorVoiceoverCancel).toHaveBeenCalledWith(1);
      expect(editorVoiceoverStop).not.toHaveBeenCalled();
      expect(queryByTestId("ed-vo-panel")).toBeNull();
    });

    it("a music import runs once at a time and says so", async () => {
      openDialog.mockResolvedValue("/Users/ana/Music/Long.flac");
      let finish = (_v: { file: string; durationMs: number }) => undefined as void;
      editorImportAsset.mockImplementation(() => new Promise((r) => (finish = r)));
      const { getByTestId, queryByTestId } = await mount();
      await act(async () => {
        fireEvent.keyDown(window, { key: "m" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(getByTestId("ed-music-busy")).toBeTruthy();
      await act(async () => {
        fireEvent.keyDown(window, { key: "m" });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(openDialog).toHaveBeenCalledTimes(1);
      await act(async () => {
        finish({ file: "f00d.flac", durationMs: 90_000 });
        await vi.advanceTimersByTimeAsync(5);
      });
      expect(queryByTestId("ed-music-busy")).toBeNull();
      expect((lastSent() as RecordEditDoc).audioTracks).toHaveLength(1);
    });

    it("the preview loads only plain file names under assets/", async () => {
      const bad = { ...voTrack, id: "x", file: "../../etc/x.wav" };
      editorCurrent.mockResolvedValue(init(null, 1, "Take", viewOf({ ...doc0, audioTracks: [voTrack, bad] })));
      const { container } = await mount();
      const srcs = [...container.querySelectorAll('[data-testid="ed-audio-track"]')].map((e) => e.getAttribute("src"));
      expect(srcs).toEqual(["asset:///m/take/assets/voiceover-1.wav"]);
    });

    it("the transport mic slider moves in 0.5 dB like the Audio tab", async () => {
      const { getAllByLabelText } = await mount();
      const s = getAllByLabelText("Mic").find((e) => (e as HTMLInputElement).type === "range") as HTMLInputElement;
      expect(s.step).toBe("0.5");
      await act(async () => {
        fireEvent.change(s, { target: { value: "-2.5" } });
        fireEvent.pointerUp(s);
        await vi.advanceTimersByTimeAsync(5);
      });
      expect((lastSent() as RecordEditDoc).micGainDb).toBe(-2.5);
    });

    for (const signal of ["ended", "finished"] as const) {
      it(`a take that ends by itself is collected and shows its track (${signal})`, async () => {
        editorVoiceoverStop.mockResolvedValue(viewOf({ ...doc0, audioTracks: [voTrack] }));
        const { getByTestId, queryByTestId } = await mount();
        await act(async () => {
          fireEvent.keyDown(window, { key: "v" });
          await vi.advanceTimersByTimeAsync(5);
        });
        await act(async () => {
          if (signal === "ended") listeners.get("record-voiceover-level")?.({ payload: { rms: 0, elapsedMs: 6000, ended: true } });
          // A second signal for the same take collects nothing more.
          else for (let i = 0; i < 2; i += 1) listeners.get("record-voiceover-finished")?.({ payload: { sessionId: 1 } });
          await vi.advanceTimersByTimeAsync(5);
        });
        expect(editorVoiceoverStop).toHaveBeenCalledTimes(1);
        expect(queryByTestId("ed-vo-panel")).toBeNull();
        expect(getByTestId("ed-audio-bar")).toBeTruthy();
      });
    }

    it("waveforms arriving later are drawn without an edit", async () => {
      const { queryAllByTestId } = await mount();
      expect(queryAllByTestId("ed-wave")).toHaveLength(0);
      await act(async () => {
        listeners.get("record-editor-waveforms")?.({
          payload: { sessionId: 1, waveforms: [{ kind: "mic", clip: 0, track: null, repeat: 0, progStartMs: 0, progEndMs: 10_000, peaks: [-0.5, 0.5, -0.2, 0.3] }] },
        });
      });
      expect(queryAllByTestId("ed-wave")).toHaveLength(1);
      // Another session's waveforms are ignored.
      await act(async () => {
        listeners.get("record-editor-waveforms")?.({ payload: { sessionId: 9, waveforms: [] } });
      });
      expect(queryAllByTestId("ed-wave")).toHaveLength(1);
    });
  });
});

describe("RecordEditor project switch (⌘O)", () => {
  const item = (dir: string, title: string, updatedAt: number) => ({
    dir, title, createdAt: 1, updatedAt, durationMs: 5000, status: "ready", failure: null, active: false, thumb: null,
  });

  it("⌘O lists the projects; picking one waits for pending edits, then opens it from the editor", async () => {
    projects.mockResolvedValue([item("/m/take", "Take", 30), item("/m/other", "Other one", 20), item("/m/third", "Third", 10)]);
    let release: () => void = () => {};
    editorUpdate.mockImplementationOnce((d: RecordEditDoc) => new Promise((res) => (release = () => res(viewOf(d)))));
    const { getByTestId, getByText, queryByTestId } = await mount();
    // An edit is still on its way to Rust.
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "o", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(getByTestId("ed-projects")).toBeTruthy();
    expect(getByTestId("ed-projects").textContent).toContain("open now");
    // While the dialog is open, editor keys do nothing (typing goes to its search field).
    const splits = editorUpdate.mock.calls.length;
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate.mock.calls.length).toBe(splits);
    await act(async () => {
      fireEvent.click(getByText("Other one"));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(queryByTestId("ed-projects")).toBeNull();
    expect(editorOpen, "not before the pending edit landed").not.toHaveBeenCalled();
    await act(async () => {
      release();
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorOpen).toHaveBeenCalledWith("/m/other", { fromEditor: true });
  });

  it("the dialog searches and opens with the keys; Esc closes it; the open project is not reopened", async () => {
    projects.mockResolvedValue([item("/m/take", "Take", 30), item("/m/b", "Überblick", 20), item("/m/c", "Demo", 10)]);
    const { getByTestId, getByLabelText, queryByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "o", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.change(getByLabelText("Search by name or date"), { target: { value: "uber" } });
    });
    expect(getByTestId("ed-projects").textContent).toContain("Überblick");
    expect(getByTestId("ed-projects").textContent).not.toContain("Demo");
    await act(async () => {
      fireEvent.keyDown(window, { key: "Enter" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorOpen).toHaveBeenCalledWith("/m/b", { fromEditor: true });
    editorOpen.mockClear();
    await act(async () => {
      fireEvent.keyDown(window, { key: "o", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "Enter" }); // the open project (first, most recent)
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorOpen).not.toHaveBeenCalled();
    expect(queryByTestId("ed-projects")).toBeNull();
    await act(async () => {
      fireEvent.keyDown(window, { key: "o", metaKey: true });
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(queryByTestId("ed-projects")).toBeNull();
  });
});

describe("RecordEditor shortcuts", () => {
  it("a rebound split: S does nothing any more, X splits", async () => {
    getRecordShortcuts.mockResolvedValueOnce({ split: { key: "x" } });
    await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.keyDown(window, { key: "x" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).clips).toHaveLength(2);
  });

  it("a map saved in another window applies at once", async () => {
    await mount();
    await act(async () => {
      listeners.get("record-shortcuts-changed")?.({ payload: { addZoom: { key: "q" }, split: { key: "z" } } });
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "z" });
      await vi.advanceTimersByTimeAsync(5);
    });
    const sent = lastSent() as RecordEditDoc;
    expect(sent.clips).toHaveLength(2);
    expect(sent.zooms).toHaveLength(0);
  });

  it("? opens the shortcuts dialog; while it is open no editor key acts; a change is saved as deviations", async () => {
    const { getByTestId, queryByTestId } = await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "?", shiftKey: true });
    });
    expect(getByTestId("ed-shortcuts")).toBeTruthy();
    await act(async () => {
      fireEvent.keyDown(window, { key: "s" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(editorUpdate).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(getByTestId("ed-key-split"));
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "x" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(setRecordShortcuts).toHaveBeenCalledWith({ split: { key: "x" } });
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(queryByTestId("ed-shortcuts")).toBeNull();
    await act(async () => {
      fireEvent.keyDown(window, { key: "x" });
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).clips).toHaveLength(2);
  });

  it("Enter on a focused button presses the button, even when Enter is bound to an action", async () => {
    getRecordShortcuts.mockResolvedValueOnce({ split: { key: "Enter" } });
    const { getByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    const button = getByTestId("ed-inspector-toggle");
    button.focus();
    let ev!: KeyboardEvent;
    await act(async () => {
      ev = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      button.dispatchEvent(ev);
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(ev.defaultPrevented).toBe(false);
    expect(editorUpdate).not.toHaveBeenCalled();
    await act(async () => {
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
      await vi.advanceTimersByTimeAsync(5);
    });
    expect((lastSent() as RecordEditDoc).clips).toHaveLength(2);
  });

  it("an action that lost its key to a stored one shows as not assigned in the dialog", async () => {
    getRecordShortcuts.mockResolvedValueOnce({ addZoom: { key: "s" } });
    const { getByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    await act(async () => {
      fireEvent.click(getByTestId("ed-shortcuts-open"));
    });
    expect(getByTestId("ed-key-split").textContent).toBe("not assigned");
    expect(getByTestId("ed-key-addZoom").textContent).toBe("S");
  });

  it("empty lanes name the key that fills them, in the current map", async () => {
    getRecordShortcuts.mockResolvedValueOnce({ addZoom: { key: "q" } });
    const { getAllByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    const hints = getAllByTestId("ed-lane-hint").map((e) => e.textContent);
    expect(hints).toContain("Q: add zoom");
    expect(hints).toContain("A: add annotation");
    expect(hints).toContain("M: music · V: voiceover");
  });

  it("an unassigned action's lane has no hint", async () => {
    getRecordShortcuts.mockResolvedValueOnce({ addAudio: null });
    const { getAllByTestId } = await mount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5);
    });
    expect(getAllByTestId("ed-lane-hint").map((e) => e.textContent)).toContain("V: voiceover");
  });
});
