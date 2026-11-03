// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAnnotation, RecordAnnotationBody, RecordEditDoc, RecordEditorView, RecordZoomRegion } from "../../lib/ipc";
import { defaultAnnotationBody } from "../../lib/editor";

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
    version: 3,
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
    annotations: [],
    audioTracks: [],
    systemGainDb: 0,
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

describe("Timeline clip reorder", () => {
  /** Three clips: 0–3 s, 3–6 s, 6–10 s (1 px = 10 ms). */
  function setup3() {
    const doc = {
      ...doc0(),
      clips: [
        { startMs: 0, endMs: 3_000 },
        { startMs: 3_000, endMs: 6_000 },
        { startMs: 6_000, endMs: 10_000 },
      ],
      crop: [null, null, null],
    } as RecordEditDoc;
    const view: RecordEditorView = {
      ...view0(doc),
      clips: [
        { progStartMs: 0, progEndMs: 3_000 },
        { progStartMs: 3_000, progEndMs: 6_000 },
        { progStartMs: 6_000, progEndMs: 10_000 },
      ],
    };
    const onMoveClip = vi.fn();
    const onSelect = vi.fn();
    render(
      <Timeline
        view={view}
        thumbs={[]}
        positionMs={0}
        durationMs={10_000}
        selectedClip={null}
        onSelect={onSelect}
        onSeek={() => undefined}
        getDoc={() => doc}
        onChange={() => undefined}
        onError={() => undefined}
        onSpeedValue={() => undefined}
        onSpeedRemove={() => undefined}
        onWidth={() => undefined}
        onMoveClip={onMoveClip}
      />,
    );
    return { onMoveClip, onSelect, clips: screen.getAllByTestId("ed-clip") };
  }

  it("dragging the first clip over the two others moves it to the end", () => {
    const { onMoveClip, clips } = setup3();
    // Middle at 1.5 s + 800 px (8 s) = 9.5 s: past the middles of clip 1 (4.5 s) and 2 (8 s).
    fireEvent.pointerDown(clips[0], { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(clips[0], { clientX: 950, pointerId: 1 });
    expect(screen.getByTestId("ed-clip-drop")).toBeTruthy();
    fireEvent.pointerUp(clips[0], { clientX: 950, pointerId: 1 });
    expect(onMoveClip).toHaveBeenCalledTimes(1);
    expect(onMoveClip).toHaveBeenCalledWith(0, 2);
  });

  it("dragging the last clip to the front moves it to index 0", () => {
    const { onMoveClip, clips } = setup3();
    fireEvent.pointerDown(clips[2], { clientX: 800, pointerId: 1 });
    fireEvent.pointerMove(clips[2], { clientX: 50, pointerId: 1 });
    fireEvent.pointerUp(clips[2], { clientX: 50, pointerId: 1 });
    expect(onMoveClip).toHaveBeenCalledWith(2, 0);
  });

  it("a drop into its own gap or a click moves nothing", () => {
    const { onMoveClip, onSelect, clips } = setup3();
    fireEvent.pointerDown(clips[1], { clientX: 450, pointerId: 1 });
    fireEvent.pointerMove(clips[1], { clientX: 480, pointerId: 1 });
    expect(screen.queryByTestId("ed-clip-drop")).toBeNull();
    fireEvent.pointerUp(clips[1], { clientX: 480, pointerId: 1 });
    fireEvent.pointerDown(clips[1], { clientX: 450, pointerId: 1 });
    fireEvent.pointerUp(clips[1], { clientX: 450, pointerId: 1 });
    expect(onMoveClip).not.toHaveBeenCalled();
    expect(onSelect).toHaveBeenCalledWith(1);
  });
});


describe("Timeline annotation lane", () => {
  const text = (id: string, clip: number, startMs: number, endMs: number, content = ""): RecordAnnotation => ({
    id,
    clip,
    startMs,
    endMs,
    x: 50,
    y: 50,
    w: 30,
    h: 20,
    zIndex: 1,
    body: { ...(defaultAnnotationBody("text") as Extract<RecordAnnotationBody, { kind: "text" }>), content },
  });

  /** Spans are Rust's answer; deliberately NOT what clip offsets would give (proves no TS math). */
  function setupAnn(
    annotations: RecordAnnotation[],
    spans: { index: number; progStartMs: number; progEndMs: number }[],
    over: { selectedAnnotation?: string | null; positionMs?: number } = {},
  ) {
    const doc = { ...doc0(), annotations } as RecordEditDoc;
    const view: RecordEditorView = { ...view0(doc), annotationSpans: spans };
    const onChange = vi.fn();
    const onSelectAnnotation = vi.fn();
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
        onError={() => undefined}
        onSpeedValue={() => undefined}
        onSpeedRemove={() => undefined}
        onWidth={() => undefined}
        selectedAnnotation={over.selectedAnnotation ?? null}
        onSelectAnnotation={onSelectAnnotation}
      />,
    );
    return { onChange, onSelectAnnotation };
  }

  it("draws one block per span where Rust placed it, labelled and coloured by kind", () => {
    const arrow: RecordAnnotation = { ...text("b", 1, 7_000, 8_000), body: defaultAnnotationBody("arrow") };
    setupAnn([text("a", 0, 1_000, 2_000, "Hello"), arrow], [
      { index: 0, progStartMs: 3_000, progEndMs: 4_000 },
      { index: 1, progStartMs: 8_200, progEndMs: 9_500 },
    ]);
    const blocks = screen.getAllByTestId("ed-ann");
    expect(blocks.map((b) => b.style.left)).toEqual(["300px", "820px"]);
    expect(blocks.map((b) => b.style.width)).toEqual(["100px", "130px"]);
    expect(blocks.map((b) => b.getAttribute("data-kind"))).toEqual(["text", "arrow"]);
    expect(blocks[0].textContent).toBe("Hello");
    expect(blocks[0].className).not.toBe(blocks[1].className);
  });

  it("overlapping annotations stack on separate rows", () => {
    setupAnn([text("a", 0, 0, 3_000), text("b", 0, 1_000, 2_000), text("c", 0, 3_500, 4_000)], [
      { index: 0, progStartMs: 0, progEndMs: 3_000 },
      { index: 1, progStartMs: 1_000, progEndMs: 2_000 },
      { index: 2, progStartMs: 3_500, progEndMs: 4_000 },
    ]);
    expect(screen.getAllByTestId("ed-ann").map((b) => b.style.top)).toEqual(["0px", "20px", "0px"]);
  });

  it("Tab walks annotations by programme start, Shift+Tab backwards, wrapping", () => {
    // List order a, b, c; programme order c (1 s), a (4 s), b (6 s).
    const anns = [text("a", 0, 0, 1_000), text("b", 0, 0, 1_000), text("c", 0, 0, 1_000)];
    const spans = [
      { index: 0, progStartMs: 4_000, progEndMs: 5_000 },
      { index: 1, progStartMs: 6_000, progEndMs: 7_000 },
      { index: 2, progStartMs: 1_000, progEndMs: 2_000 },
    ];
    const tab = (shiftKey = false) => {
      const ev = new KeyboardEvent("keydown", { key: "Tab", shiftKey, bubbles: true, cancelable: true });
      window.dispatchEvent(ev);
      return ev;
    };
    const none = setupAnn(anns, spans);
    expect(tab().defaultPrevented).toBe(true);
    expect(none.onSelectAnnotation).toHaveBeenLastCalledWith("c");
    cleanup();
    const fromA = setupAnn(anns, spans, { selectedAnnotation: "a" });
    tab();
    expect(fromA.onSelectAnnotation).toHaveBeenLastCalledWith("b");
    tab(true);
    expect(fromA.onSelectAnnotation).toHaveBeenLastCalledWith("c");
    cleanup();
    const fromB = setupAnn(anns, spans, { selectedAnnotation: "b" });
    tab();
    expect(fromB.onSelectAnnotation).toHaveBeenLastCalledWith("c");
  });

  it("Tab on a focused button keeps moving the focus", () => {
    const { onSelectAnnotation } = setupAnn([text("a", 0, 0, 1_000)], [{ index: 0, progStartMs: 0, progEndMs: 1_000 }]);
    const btn = document.createElement("button");
    document.body.appendChild(btn);
    const ev = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    btn.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
    expect(onSelectAnnotation).not.toHaveBeenCalled();
    btn.remove();
  });

  it("A adds a text annotation at the playhead and selects it", async () => {
    const { onChange, onSelectAnnotation } = setupAnn([], [], { positionMs: 7_000 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true }));
    await flush();
    const next = onChange.mock.calls[0][0] as RecordEditDoc;
    expect(next.annotations).toHaveLength(1);
    expect(next.annotations[0]).toMatchObject({ clip: 1, startMs: 8_000, endMs: 10_000 });
    expect(next.annotations[0].body.kind).toBe("text");
    expect(onSelectAnnotation).toHaveBeenCalledWith(next.annotations[0].id);
  });

  it("Delete removes the selected annotation (and nothing else)", () => {
    const { onChange, onSelectAnnotation } = setupAnn([text("a", 0, 0, 1_000), text("b", 0, 0, 1_000)], [], { selectedAnnotation: "a" });
    const ev = new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true });
    window.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect((onChange.mock.calls[0][0] as RecordEditDoc).annotations.map((a) => a.id)).toEqual(["b"]);
    expect(onSelectAnnotation).toHaveBeenCalledWith(null);
  });

  it("dragging a block moves the annotation in its clip (no origin field)", async () => {
    const { onChange, onSelectAnnotation } = setupAnn([text("a", 0, 1_000, 2_000)], [{ index: 0, progStartMs: 1_000, progEndMs: 2_000 }]);
    const block = screen.getByTestId("ed-ann");
    fireEvent.pointerDown(block, { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 250, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 250, pointerId: 1 });
    await flush();
    expect(onSelectAnnotation).toHaveBeenCalledWith("a");
    const a = (onChange.mock.calls[0][0] as RecordEditDoc).annotations[0];
    expect(a).toMatchObject({ startMs: 2_000, endMs: 3_000 });
    expect("origin" in a).toBe(false);
  });
});
