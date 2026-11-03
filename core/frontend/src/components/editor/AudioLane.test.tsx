// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAudioTrack, RecordEditDoc, RecordEditorView, RecordTrackSpan, RecordWaveSpan } from "../../lib/ipc";

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
function mount(d: RecordEditDoc, tracks: RecordTrackSpan[], over: { locked?: boolean; waveforms?: RecordWaveSpan[] } = {}) {
  const onChange = vi.fn();
  const onSelect = vi.fn();
  const onError = vi.fn();
  render(
    <AudioLane
      view={view(d, tracks, over.waveforms)}
      scale={0.1}
      getDoc={() => d}
      onChange={onChange}
      onError={onError}
      selected={null}
      onSelect={onSelect}
      locked={over.locked}
    />,
  );
  return { onChange, onSelect, onError };
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
    expect(dragTrackBar(bar, track(), "move", 1_500, 10_000)).toMatchObject({ progStartMs: 3_500, progEndMs: 6_500 });
    expect(dragTrackBar(bar, track(), "move", -9_000, 10_000)).toMatchObject({ progStartMs: 0, progEndMs: 3_000 });
    expect(dragTrackBar(bar, track(), "move", 9_000, 10_000)).toMatchObject({ progStartMs: 7_000, progEndMs: 10_000 });
  });
  it("the start edge eats into the file and never before its first sample", () => {
    expect(dragTrackBar(bar, track(), "start", 500, 10_000)).toMatchObject({ progStartMs: 2_500, offsetMs: 500, spanMs: null });
    expect(dragTrackBar(bar, track({ offsetMs: 400 }), "start", -1_000, 10_000)).toMatchObject({ progStartMs: 1_600, offsetMs: 0 });
    // A recorded span shrinks with it; the start never passes the end.
    expect(dragTrackBar(bar, track({ spanMs: 3_000 }), "start", 9_000, 10_000)).toMatchObject({
      progStartMs: 5_000 - MIN_TRACK_MS,
      spanMs: MIN_TRACK_MS,
    });
  });
  it("the end edge sets the played length, up to the file's end (or the programme's, looping)", () => {
    expect(dragTrackBar(bar, track(), "end", -1_000, 10_000)).toMatchObject({ progEndMs: 4_000, spanMs: 2_000 });
    expect(dragTrackBar(bar, track(), "end", 5_000, 10_000)).toMatchObject({ progEndMs: 5_000, spanMs: 3_000 });
    expect(dragTrackBar(bar, track({ looped: true }), "end", 9_000, 10_000)).toMatchObject({ progEndMs: 10_000, spanMs: 8_000 });
    expect(dragTrackBar(bar, track(), "end", -9_000, 10_000)).toMatchObject({ progEndMs: 2_000 + MIN_TRACK_MS });
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
    expect(onChange.mock.calls[0][0].audioTracks[0]).toMatchObject({ clip: 1, startMs: 7_000, offsetMs: 0 });
  });

  it("a drag past the programme end is clamped there", async () => {
    const { onChange } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-bar"), 5_000);
    expect(toSource).toHaveBeenCalledWith(7_000);
    expect(onChange).toHaveBeenCalledTimes(1);
    await dragBy(screen.getByTestId("ed-audio-bar"), -5_000);
    expect(toSource).toHaveBeenLastCalledWith(0);
  });

  it("the start edge trims the file's start; the end edge the length", async () => {
    const { onChange } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })]);
    await dragBy(screen.getByTestId("ed-audio-start"), 50);
    expect(onChange.mock.calls[0][0].audioTracks[0]).toMatchObject({ offsetMs: 500, clip: 0, startMs: 2_500 });
    await dragBy(screen.getByTestId("ed-audio-end"), -100);
    expect(onChange.mock.calls[1][0].audioTracks[0]).toMatchObject({ spanMs: 2_000, startMs: 1_000 });
  });

  it("a click selects without editing; nothing drags while a take runs", async () => {
    const { onChange, onSelect } = mount(doc([track()]), [entry({ progStartMs: 2_000, progEndMs: 5_000 })], { locked: true });
    await dragBy(screen.getByTestId("ed-audio-bar"), 400);
    expect(onSelect).toHaveBeenCalledWith("m1");
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.queryByTestId("ed-audio-start")).toBeNull();
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
