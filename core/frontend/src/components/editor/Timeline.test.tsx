// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordEditDoc, RecordEditorView, RecordZoomRegion } from "../../lib/ipc";

const toSource = vi.fn();
vi.mock("../../lib/ipc", () => ({ editorToSource: (p: number) => toSource(p) }));

const { Timeline } = await import("./Timeline");

/** The track measures 1000 px: 1 px = 10 ms on a 10-s programme. */
class FakeResizeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe() {
    this.cb([{ contentRect: { width: 1000 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  disconnect() {}
  unobserve() {}
}

const zoom = (id: string, clip: number, startMs: number, endMs: number, origin: "auto" | "manual"): RecordZoomRegion => ({
  id,
  clip,
  startMs,
  endMs,
  depth: 3,
  customScale: null,
  focusX: 0.5,
  focusY: 0.5,
  focusMode: "auto",
  origin,
  hideCursor: false,
});

const doc0 = (zooms: RecordZoomRegion[] = []): RecordEditDoc =>
  ({
    version: 2,
    clips: [
      { startMs: 0, endMs: 5_000 },
      { startMs: 6_000, endMs: 11_000 },
    ],
    speed: [],
    crop: [null, null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {} as RecordEditDoc["appearance"],
    cursor: {} as RecordEditDoc["cursor"],
    zooms,
    autoZoomDone: true,
  }) as RecordEditDoc;

/** Programme spans: clip 0 = 0–5 s, clip 1 = 5–10 s (source 6–11 s). */
const view0 = (doc: RecordEditDoc): RecordEditorView => ({
  doc,
  programmeMs: 10_000,
  clips: [
    { progStartMs: 0, progEndMs: 5_000 },
    { progStartMs: 5_000, progEndMs: 10_000 },
  ],
  speed: [],
  zooms: doc.zooms.map((z, index) => {
    const off = z.clip === 0 ? 0 : -1_000;
    return { index, progStartMs: z.startMs + off, progEndMs: z.endMs + off };
  }),
  clickHits: [],
  screenRects: [],
  outputSize: [1920, 1080],
});

function setup(doc: RecordEditDoc, over: { positionMs?: number; selectedZoom?: string | null } = {}) {
  const onChange = vi.fn();
  const onSelectZoom = vi.fn();
  const onError = vi.fn();
  const view = view0(doc);
  render(
    <Timeline
      view={view}
      thumbs={[]}
      positionMs={over.positionMs ?? 0}
      durationMs={11_000}
      selectedClip={null}
      onSelect={() => undefined}
      onSeek={() => undefined}
      getDoc={() => doc}
      onChange={onChange}
      onError={onError}
      onSpeedValue={() => undefined}
      onSpeedRemove={() => undefined}
      onWidth={() => undefined}
      selectedZoom={over.selectedZoom ?? null}
      onSelectZoom={onSelectZoom}
    />,
  );
  return { onChange, onSelectZoom, onError };
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  toSource.mockReset();
  // Clip 0 plays source = programme; clip 1 starts at programme 5 s = source 6 s.
  toSource.mockImplementation(async (p: number) => (p < 5_000 ? { clip: 0, sourceMs: p } : { clip: 1, sourceMs: p + 1_000 }));
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const flush = () => act(async () => {
  await new Promise((r) => setTimeout(r, 0));
});

describe("Timeline zoom track", () => {
  it("Z adds a manual zoom at the playhead", async () => {
    const { onChange, onSelectZoom } = setup(doc0(), { positionMs: 7_000 });
    const ev = new KeyboardEvent("keydown", { key: "z", bubbles: true, cancelable: true });
    window.dispatchEvent(ev);
    await flush();
    expect(ev.defaultPrevented).toBe(true);
    expect(toSource).toHaveBeenCalledWith(7_000);
    const next = onChange.mock.calls[0][0] as RecordEditDoc;
    expect(next.zooms).toHaveLength(1);
    expect(next.zooms[0]).toMatchObject({ clip: 1, startMs: 8_000, origin: "manual" });
    expect(onSelectZoom).toHaveBeenCalledWith(next.zooms[0].id);
  });

  it("Delete removes the selected zoom (and nothing else)", () => {
    const doc = doc0([zoom("a", 0, 1_000, 2_000, "manual"), zoom("b", 0, 3_000, 4_000, "auto")]);
    const { onChange } = setup(doc, { selectedZoom: "a" });
    const ev = new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true });
    window.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect((onChange.mock.calls[0][0] as RecordEditDoc).zooms.map((z) => z.id)).toEqual(["b"]);
  });

  it("without a selected zoom Delete is left to the editor (clip delete)", () => {
    const { onChange } = setup(doc0([zoom("a", 0, 1_000, 2_000, "manual")]));
    const ev = new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true });
    window.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("marks auto and manual zooms differently", () => {
    setup(doc0([zoom("a", 0, 1_000, 2_000, "manual"), zoom("b", 0, 3_000, 4_000, "auto")]));
    const blocks = screen.getAllByTestId("ed-zoom");
    expect(blocks.map((b) => b.getAttribute("data-origin"))).toEqual(["manual", "auto"]);
    expect(blocks[0].className).not.toBe(blocks[1].className);
  });

  it("a drag past the clip edge clamps inside the clip", async () => {
    const doc = doc0([zoom("a", 0, 1_000, 2_000, "manual")]);
    const { onChange, onSelectZoom } = setup(doc);
    const block = screen.getByTestId("ed-zoom");
    // 1 px = 10 ms: drag 900 px = 9 s to the right, far into clip 1.
    fireEvent.pointerDown(block, { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 1_050, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 1_050, pointerId: 1 });
    await flush();
    expect(onSelectZoom).toHaveBeenCalledWith("a");
    const z = (onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as RecordEditDoc).zooms[0];
    expect(z.clip).toBe(0);
    expect(z.endMs).toBe(5_000);
    expect(z.startMs).toBe(4_000);
    for (const [p] of toSource.mock.calls) expect(p).toBeLessThan(5_000);
  });

  it("a failing source lookup during a zoom drag is reported, not swallowed", async () => {
    const { onChange, onError } = setup(doc0([zoom("a", 0, 1_000, 2_000, "manual")]));
    toSource.mockRejectedValue("boom");
    const block = screen.getByTestId("ed-zoom");
    fireEvent.pointerDown(block, { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 250, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 250, pointerId: 1 });
    await flush();
    expect(onError).toHaveBeenCalledWith("boom");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("moving or trimming a planned zoom makes it the user's", async () => {
    const doc = doc0([zoom("a", 0, 1_000, 2_000, "auto")]);
    const { onChange } = setup(doc);
    const block = screen.getByTestId("ed-zoom");
    fireEvent.pointerDown(block, { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 250, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 250, pointerId: 1 });
    await flush();
    expect((onChange.mock.calls[0][0] as RecordEditDoc).zooms[0]).toMatchObject({ startMs: 2_000, endMs: 3_000, origin: "manual" });
    for (const edge of ["Zoom start", "Zoom end"]) {
      onChange.mockClear();
      const handle = screen.getByLabelText(edge);
      fireEvent.pointerDown(handle, { clientX: 100, pointerId: 1 });
      fireEvent.pointerMove(handle, { clientX: 150, pointerId: 1 });
      fireEvent.pointerUp(handle, { clientX: 150, pointerId: 1 });
      await flush();
      expect((onChange.mock.calls[0][0] as RecordEditDoc).zooms[0].origin, edge).toBe("manual");
    }
  });

  it("a click without movement only selects", async () => {
    const { onChange, onSelectZoom } = setup(doc0([zoom("a", 0, 1_000, 2_000, "manual")]));
    const block = screen.getByTestId("ed-zoom");
    fireEvent.pointerDown(block, { clientX: 150, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 150, pointerId: 1 });
    await flush();
    expect(onSelectZoom).toHaveBeenCalledWith("a");
    expect(onChange).not.toHaveBeenCalled();
  });
});
