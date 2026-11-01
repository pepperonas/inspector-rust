import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import type { EditorInit, RecordEditDoc, RecordEditorView } from "../lib/ipc";

const editorCurrent = vi.fn();
const editorUpdate = vi.fn();
const editorSeek = vi.fn();
const editorPlay = vi.fn();
const editorPosition = vi.fn();
const editorFrame = vi.fn();
const editorResize = vi.fn();
const editorThumbs = vi.fn();
const editorRestart = vi.fn();
const listeners = new Map<string, (e: { payload: unknown }) => void>();

vi.mock("../lib/ipc", () => ({
  editorCurrent: () => editorCurrent(),
  editorUpdate: (d: RecordEditDoc) => editorUpdate(d),
  editorSeek: (t: number) => editorSeek(t),
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
  return { doc, programmeMs: t, clips, speed: [] };
};
const init = (helperError: string | null = null): EditorInit => ({
  project: { dir: "/m/take", title: "Take", createdAt: 0, durationMs: 10_000, status: "ready", failure: null },
  meta: {
    source: "display", sourceLabel: "D", width: 1920, height: 1080, fps: 30, durationMs: 10_000,
    systemAudio: true, mic: { offsetMs: 0, sampleRate: 48_000 }, cursorMode: "hidden" as never, pauses: [],
  },
  view: viewOf(doc0),
  helperError,
});
const playback = (playing: boolean, positionMs = 0) => ({
  playing, positionMs, programmeMs: 10_000, exporting: false, helperError: null,
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
    expect(editorSeek).toHaveBeenCalled();
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
});
