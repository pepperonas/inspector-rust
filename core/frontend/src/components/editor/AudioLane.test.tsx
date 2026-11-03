// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAudioTrack, RecordEditDoc, RecordEditorView, RecordTrackSpan, RecordWaveSpan } from "../../lib/ipc";
import { CAPTION_DEFAULTS } from "../../lib/editor";

const toSource = vi.fn();
vi.mock("../../lib/ipc", () => ({ editorToSource: (p: number) => toSource(p) }));

const { AudioLane, dragTrackBar, trackBars, Waveform, MIN_TRACK_MS } = await import("./AudioLane");

afterEach(cleanup);
beforeEach(() => {
  toSource.mockReset();
  toSource.mockImplementation(async (p: number) => ({ clip: p >= 5_000 ? 1 : 0, sourceMs: p >= 5_000 ? p + 1_000 : p }));
});

const track = (over: Partial<RecordAudioTrack> = {}): RecordAudioTrack => ({
  id: "m1",
  kind: "music",
  file: "song.m4a",
  clip: 0,
  startMs: 1_000,
  durationMs: 3_000,
  offsetMs: 0,
  spanMs: null,
  gainDb: -18,
  looped: false,
  fadeInMs: 1_000,
  fadeOutMs: 1_000,
  muted: false,
  label: "",
  ...over,
});

const entry = (over: Partial<RecordTrackSpan>): RecordTrackSpan => ({
  index: 0,
  repeat: 0,
  progStartMs: 0,
  progEndMs: 1_000,
  fileStartMs: 0,
  fileEndMs: 1_000,
  fadeInMs: 0,
  fadeOutMs: 0,
  gainDb: 0,
  kind: "music",
  muted: false,
  ...over,
});

const doc = (audioTracks: RecordAudioTrack[]): RecordEditDoc =>
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
    zooms: [],
    autoZoomDone: true,
    annotations: [],
    audioTracks,
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
    transcripts: [],
    captions: { ...CAPTION_DEFAULTS },
  }) as RecordEditDoc;

const view = (d: RecordEditDoc, tracks: RecordTrackSpan[], waveforms: RecordWaveSpan[] = []): RecordEditorView => ({
  doc: d,
  programmeMs: 10_000,
  clips: [
    { progStartMs: 0, progEndMs: 5_000 },
    { progStartMs: 5_000, progEndMs: 10_000 },
  ],
  speed: [],
  zooms: [],
  clickHits: [],
  screenRects: [],
  outputSize: [1920, 1080],
  tracks,
  waveforms,
});

/** 1 px = 10 ms. */
function mount(d: RecordEditDoc, tracks: RecordTrackSpan[], over: { noEdges?: boolean; waveforms?: RecordWaveSpan[] } = {}) {
  const onChange = vi.fn();
  const onSelect = vi.fn();
  const onError = vi.fn();
  // Stands in for Rust's `editor_track_edge` (the file window is Rust's to work out).
  const resolveEdge = vi.fn(async (_i: number, _id: string, edge: "start" | "end", p: number) =>
    edge === "start" ? { clip: 1, startMs: p + 1_000, offsetMs: 4_500, spanMs: 1_500 } : { clip: 0, startMs: 1_000, offsetMs: 0, spanMs: p },
  );
  render(
    <AudioLane
      view={view(d, tracks, over.waveforms)}
      scale={0.1}
      getDoc={() => d}
      onChange={onChange}
      onError={onError}
      selected={null}
      onSelect={onSelect}
      resolveEdge={over.noEdges ? undefined : resolveEdge}
    />,
  );
  return { onChange, onSelect, onError, resolveEdge };
}

async function dragBy(el: Element, dx: number) {
  fireEvent.pointerDown(el, { clientX: 300, pointerId: 1 });
  fireEvent.pointerMove(el, { clientX: 300 + dx, pointerId: 1 });
  fireEvent.pointerUp(el, { clientX: 300 + dx, pointerId: 1 });
  // The commit awaits Rust's source lookup.
  await act(async () => {
    await Promise.resolve();
  });
}

describe("trackBars", () => {
  it("one bar per track from Rust's entries; loop repeats are marked", () => {
    const v = view(doc([track(), track({ id: "v1", kind: "voiceover" })]), [
      entry({ index: 0, repeat: 0, progStartMs: 2_000, progEndMs: 5_000 }),
      entry({ index: 0, repeat: 1, progStartMs: 5_000, progEndMs: 8_000 }),
      entry({ index: 0, repeat: 2, progStartMs: 8_000, progEndMs: 9_500 }),
      entry({ index: 1, progStartMs: 100, progEndMs: 900 }),
    ]);
    expect(trackBars(v)).toEqual([
      { index: 0, progStartMs: 2_000, progEndMs: 9_500, repeats: [5_000, 8_000], muted: false },
      { index: 1, progStartMs: 100, progEndMs: 900, repeats: [], muted: false },
    ]);
  });
});

describe("dragTrackBar", () => {
  const bar = { progStartMs: 2_000, progEndMs: 5_000 };
  it("moving keeps the length and is clamped to the programme", () => {
    expect(dragTrackBar(bar, track(), "move", 1_500, 10_000)).toEqual({ progStartMs: 3_500, progEndMs: 6_500 });
    expect(dragTrackBar(bar, track(), "move", -9_000, 10_000)).toEqual({ progStartMs: 0, progEndMs: 3_000 });
    expect(dragTrackBar(bar, track(), "move", 9_000, 10_000)).toEqual({ progStartMs: 7_000, progEndMs: 10_000 });
  });
  it("a loop filling to the end can still move later (Rust refills it)", () => {
    const loop = { progStartMs: 2_000, progEndMs: 10_000 };
    expect(dragTrackBar(loop, track({ looped: true }), "move", 3_000, 10_000)).toEqual({ progStartMs: 5_000, progEndMs: 10_000 });
    expect(dragTrackBar(loop, track({ looped: true }), "move", 99_000, 10_000).progStartMs).toBe(10_000 - MIN_TRACK_MS);
  });
  it("an edge moves alone, inside the programme, leaving 100 ms", () => {
    expect(dragTrackBar(bar, track(), "start", 500, 10_000)).toEqual({ progStartMs: 2_500, progEndMs: 5_000 });
    expect(dragTrackBar(bar, track(), "start", -9_000, 10_000)).toEqual({ progStartMs: 0, progEndMs: 5_000 });
    expect(dragTrackBar(bar, track(), "start", 9_000, 10_000).progStartMs).toBe(5_000 - MIN_TRACK_MS);
    expect(dragTrackBar(bar, track(), "end", 9_000, 10_000)).toEqual({ progStartMs: 2_000, progEndMs: 10_000 });
    expect(dragTrackBar(bar, track(), "end", -9_000, 10_000).progEndMs).toBe(2_000 + MIN_TRACK_MS);
  });
});

describe("dragTrackBar snapping (1 px = 10 ms, targets 0 / 5 s / 10 s)", () => {
  const bar = { progStartMs: 2_000, progEndMs: 5_000 };
  const snapping = { targets: [0, 5_000, 10_000], pxPerMs: 0.1 };
  it("a moved bar snaps with its nearer edge; an edge snaps alone", () => {
    expect(dragTrackBar(bar, track(), "move", 4_930, 10_000, snapping)).toEqual({ progStartMs: 7_000, progEndMs: 10_000 });
    expect(dragTrackBar(bar, track(), "move", 4_910, 10_000, snapping)).toEqual({ progStartMs: 6_910, progEndMs: 9_910 });
    expect(dragTrackBar(bar, track(), "end", 4_940, 10_000, snapping)).toEqual({ progStartMs: 2_000, progEndMs: 10_000 });
    expect(dragTrackBar(bar, track(), "start", 2_950, 10_000, snapping)).toEqual({ progStartMs: 5_000 - MIN_TRACK_MS, progEndMs: 5_000 });
  });
  it("a looping bar snaps only its start (its end is Rust's refill)", () => {
    const loop = { progStartMs: 2_000, progEndMs: 10_000 };
    expect(dragTrackBar(loop, track({ looped: true }), "move", 2_950, 10_000, snapping)).toEqual({ progStartMs: 5_000, progEndMs: 10_000 });
  });
});

describe("AudioLane", () => {
  it("draws its bars from Rust's spans, not the document's anchors", () => {
    // The anchor says 1 s; Rust's entry (e.g. after a speed region) says 2 s.
    mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    const bar = screen.getByTestId("ed-audio-bar") as HTMLElement;
    expect(bar.style.left).toBe("200px");
    expect(bar.style.width).toBe("300px");
    expect(bar.textContent).toContain("song");
  });

  it("marks loop repeats and draws the track's waveform", () => {
    mount(
      doc([track({ looped: true })]),
      [entry({ repeat: 0, progStartMs: 0, progEndMs: 3_000 }), entry({ repeat: 1, progStartMs: 3_000, progEndMs: 6_000 })],
      { waveforms: [{ kind: "track", clip: null, track: 0, repeat: 0, progStartMs: 0, progEndMs: 3_000, peaks: [-0.4, 0.6, -0.1, 0.2, -0.3, 0.3] }] },
    );
    expect(screen.getAllByTestId("ed-audio-repeat")).toHaveLength(1);
    expect(screen.getAllByTestId("ed-wave")).toHaveLength(1);
  });

  it("dragging moves the track; Rust maps the clamped programme start to the anchor", async () => {
    const { onChange } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-bar"), 400);
    expect(toSource).toHaveBeenCalledWith(6_000);
    expect(onChange.mock.calls[0][0].audioTracks[0]).toMatchObject({ clip: 1, startMs: 7_000, offsetMs: 0, spanMs: null });
  });

  it("a drag past the programme end is clamped there", async () => {
    const { onChange } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-bar"), 5_000);
    expect(toSource).toHaveBeenCalledWith(7_000);
    expect(onChange).toHaveBeenCalledTimes(1);
    await dragBy(screen.getByTestId("ed-audio-bar"), -5_000);
    expect(toSource).toHaveBeenLastCalledWith(0);
  });

  it("edge drags go to Rust as programme positions; its file window is applied as is", async () => {
    const { onChange, resolveEdge } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-start"), 50);
    expect(resolveEdge).toHaveBeenLastCalledWith(0, "m1", "start", 2_500);
    expect(onChange.mock.calls[0][0].audioTracks[0]).toMatchObject({ clip: 1, startMs: 3_500, offsetMs: 4_500, spanMs: 1_500 });
    await dragBy(screen.getByTestId("ed-audio-end"), -100);
    expect(resolveEdge).toHaveBeenLastCalledWith(0, "m1", "end", 4_000);
    expect(onChange.mock.calls[1][0].audioTracks[0]).toMatchObject({ spanMs: 4_000, startMs: 1_000 });
    // No file math in TS: nothing but Rust's answer reaches the document.
    expect(toSource).not.toHaveBeenCalled();
  });

  it("a drag Rust has no answer for changes nothing; without a resolver there are no edges", async () => {
    const { onChange, resolveEdge } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    resolveEdge.mockResolvedValueOnce(null as never);
    await dragBy(screen.getByTestId("ed-audio-end"), -100);
    expect(onChange).not.toHaveBeenCalled();
    cleanup();
    mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })], { noEdges: true });
    expect(screen.queryByTestId("ed-audio-start")).toBeNull();
  });

  it("a click selects without editing", async () => {
    const { onChange, onSelect } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-bar"), 1);
    expect(onSelect).toHaveBeenCalledWith("m1");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("draws nothing without tracks", () => {
    mount(doc([]), []);
    expect(screen.queryByTestId("ed-audio-lane")).toBeNull();
  });
});

describe("Waveform", () => {
  it("one shape over min/max pairs, clamped to ±1", () => {
    const { container } = render(<Waveform peaks={[-2, 2, -0.5, 0.5]} />);
    const d = container.querySelector("path")?.getAttribute("d") ?? "";
    expect(d).toBe("M0 -1.000 L1 -0.500 L1 0.500 L0 1.000 Z");
  });
});
