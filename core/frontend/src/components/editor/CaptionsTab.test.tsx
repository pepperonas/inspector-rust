// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordEditDoc, RecordTranscript } from "../../lib/ipc";
import { CAPTION_DEFAULTS } from "../../lib/editor";
import { patchCaptions } from "../../lib/captions-ui";
import { CaptionsTab, type CaptionsTabProps } from "./CaptionsTab";
import { fakeTranscription } from "./test-transcription";

afterEach(cleanup);

const transcript: RecordTranscript = {
  source: null,
  lang: "de",
  words: [{ id: "w1", startMs: 100, endMs: 400, text: "Hallo" }],
};

const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 5,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {} as RecordEditDoc["appearance"],
    cursor: {} as RecordEditDoc["cursor"],
    zooms: [],
    autoZoomDone: true,
    annotations: [],
    audioTracks: [],
    systemGainDb: 0,
    camera: {} as RecordEditDoc["camera"],
    layoutRegions: [],
    transcripts: [transcript],
    captions: { ...CAPTION_DEFAULTS },
    ...over,
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, over: Partial<CaptionsTabProps> = {}) {
  const props: CaptionsTabProps = {
    doc: d,
    aspect: 16 / 9,
    tr: fakeTranscription(),
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    ...over,
  };
  render(<CaptionsTab {...props} />);
  return props;
}
const last = (f: unknown) => {
  const calls = (f as ReturnType<typeof vi.fn>).mock.calls;
  return calls[calls.length - 1]?.[0] as RecordEditDoc;
};

describe("captions tab", () => {
  it("the switch writes captions.enabled", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByLabelText("Show captions"));
    expect(last(p.onCommit).captions.enabled).toBe(true);
    cleanup();
    const q = mount(doc({ captions: { ...CAPTION_DEFAULTS, enabled: true } }));
    fireEvent.click(screen.getByLabelText("Show captions"));
    expect(last(q.onCommit).captions.enabled).toBe(false);
  });

  it("the bold look sets Oswald 60 #fde047 on a 0.85 plate", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    const c = last(p.onCommit).captions;
    expect(c).toMatchObject({ fontFamily: "Oswald", fontSize: 60, color: "#fde047", plate: true, plateColor: "#000000", plateOpacity: 0.85, bold: true });
  });

  it("choosing the light plate flips the text colour to #111111", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByRole("button", { name: "Light" }));
    const c = last(p.onCommit).captions;
    expect(c).toMatchObject({ plate: true, plateColor: "#ffffff", color: "#111111" });
    // A colour that already reads on the plate is kept.
    expect(patchCaptions(CAPTION_DEFAULTS, {}).color).toBe("#ffffff");
  });

  it("a font size outside 16–140 is clamped", () => {
    const p = mount(doc());
    const slider = screen.getByLabelText("Text size") as HTMLInputElement;
    expect(slider.min).toBe("16");
    expect(slider.max).toBe("140");
    fireEvent.change(slider, { target: { value: "500" } });
    expect(last(p.onLive).captions.fontSize).toBe(140);
    expect(patchCaptions(CAPTION_DEFAULTS, { fontSize: 3 }).fontSize).toBe(16);
    expect(patchCaptions(CAPTION_DEFAULTS, { fontSize: 999 }).fontSize).toBe(140);
    // min ≤ max words, whichever was moved.
    expect(patchCaptions(CAPTION_DEFAULTS, { minWords: 9 })).toMatchObject({ minWords: 9, maxWords: 9 });
    expect(patchCaptions(CAPTION_DEFAULTS, { maxWords: 1 })).toMatchObject({ minWords: 1, maxWords: 1 });
  });

  it("without a transcript the tab offers Transcribe with DE/EN/Auto", () => {
    const start = vi.fn();
    mount(doc({ transcripts: [] }), { tr: fakeTranscription({ start }) });
    expect(screen.getByRole("button", { name: "DE" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "EN" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Auto" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "EN" }));
    fireEvent.click(screen.getByTestId("ed-tr-go"));
    expect(start).toHaveBeenCalledWith("en");
    expect(screen.getByTestId("ed-tr-go").textContent).toBe("Transcribe");
  });

  it("a missing model shows the download button", () => {
    const downloadModel = vi.fn();
    mount(doc({ transcripts: [] }), {
      tr: fakeTranscription({ downloadModel, model: { id: "large-v3-turbo-q5", file: "m.bin", bytes: 574_041_195, installed: false } }),
    });
    expect(screen.queryByTestId("ed-tr-go")).toBeNull();
    fireEvent.click(screen.getByTestId("ed-tr-download"));
    expect(downloadModel).toHaveBeenCalledOnce();
    cleanup();
    // The run itself answered model.missing (status not yet refreshed): same card.
    mount(doc({ transcripts: [] }), { tr: fakeTranscription({ error: "model.missing" }) });
    expect(screen.getByTestId("ed-tr-download")).toBeTruthy();
  });

  it("a finished run reports skipped recordings and a partial error", () => {
    mount(doc(), {
      tr: fakeTranscription({ last: { skipped: [null, "take-2"], error: "transcribe.busy", cancelled: false } }),
    });
    const note = screen.getByTestId("ed-tr-note").textContent ?? "";
    expect(note).toContain("2 recordings have no sound");
    expect(note).toContain("Stopped early");
  });

  it("the portrait default inset shows 12.5 % until moved", () => {
    mount(doc(), { aspect: 9 / 16 });
    expect((screen.getByLabelText("Distance from the bottom") as HTMLInputElement).value).toBe("12.5");
    expect(screen.queryByTestId("ed-cap-inset-reset")).toBeNull();
  });
});
