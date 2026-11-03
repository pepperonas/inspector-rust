// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordEditDoc, RecordTranscript } from "../../lib/ipc";
import { CAPTION_DEFAULTS, restoreGap } from "../../lib/editor";
import { cutWords, setWordText } from "../../lib/transcript";
import { TranscriptTab, type TranscriptTabProps } from "./TranscriptTab";
import { fakeTranscription } from "./test-transcription";

afterEach(cleanup);

const transcript: RecordTranscript = {
  source: null,
  lang: "de",
  words: [
    { id: "w1", startMs: 100, endMs: 400, text: "Hallo" },
    { id: "w2", startMs: 500, endMs: 900, text: "liebe" },
    { id: "w3", startMs: 1000, endMs: 1400, text: "Welt" },
    { id: "w4", startMs: 4500, endMs: 5000, text: "weg" },
    { id: "w5", startMs: 6500, endMs: 7000, text: "Ende" },
  ],
};

const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 5,
    clips: [
      { startMs: 0, endMs: 4000 },
      { startMs: 6000, endMs: 10_000 },
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
    audioTracks: [],
    systemGainDb: 0,
    camera: {} as RecordEditDoc["camera"],
    layoutRegions: [],
    transcripts: [transcript],
    captions: { ...CAPTION_DEFAULTS },
    ...over,
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, over: Partial<TranscriptTabProps> = {}) {
  const props: TranscriptTabProps = {
    doc: d,
    cue: null,
    tr: fakeTranscription(),
    onCommit: vi.fn(),
    toProgramme: vi.fn(async () => 4242),
    onSeek: vi.fn(),
    onWordsSelected: vi.fn(),
    ...over,
  };
  render(<TranscriptTab {...props} />);
  return props;
}
const word = (id: string) => document.querySelector(`[data-word="${id}"]`) as HTMLElement;
const last = (f: unknown) => {
  const calls = (f as ReturnType<typeof vi.fn>).mock.calls;
  return calls[calls.length - 1]?.[0] as RecordEditDoc;
};

describe("transcript tab", () => {
  it("one block per clip", () => {
    mount(doc());
    expect(screen.getByTestId("ed-tr-clip-0").textContent).toContain("Hallo liebe Welt".split(" ").join(""));
    expect(screen.getByTestId("ed-tr-clip-1").textContent).toContain("Ende");
    expect(screen.getByTestId("ed-tr-clip-0").textContent).not.toContain("Ende");
  });

  it("a click on a word seeks to its programme time from Rust", async () => {
    const p = mount(doc());
    await act(async () => {
      fireEvent.click(word("w5"));
    });
    expect(p.toProgramme).toHaveBeenCalledWith(1, 6500);
    expect(p.onSeek).toHaveBeenCalledWith(4242);
    expect(p.onWordsSelected).toHaveBeenCalled();
  });

  it("⌫ on the selected words cuts them as one undo step", async () => {
    const d = doc();
    const p = mount(d);
    await act(async () => {
      fireEvent.click(word("w1"));
      fireEvent.click(word("w2"), { shiftKey: true });
    });
    expect(word("w1").getAttribute("aria-pressed")).toBe("true");
    expect(word("w2").getAttribute("aria-pressed")).toBe("true");
    expect(word("w3").getAttribute("aria-pressed")).toBe("false");
    const ev = new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true });
    act(() => {
      word("w2").dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(true);
    expect(p.onCommit).toHaveBeenCalledTimes(1);
    expect(last(p.onCommit)).toEqual(cutWords(d, 0, ["0:w1", "0:w2"]));
  });

  it("without a selection ⌫ is left to the editor", () => {
    const p = mount(doc());
    const ev = new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true });
    act(() => {
      screen.getByTestId("ed-transcript").dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(false);
    expect(p.onCommit).not.toHaveBeenCalled();
  });

  it("the struck-through gap restores with restoreGap", () => {
    const d = doc();
    const p = mount(d);
    const gap = screen.getByTestId("ed-tr-gap-0");
    expect(gap.textContent).toContain("weg");
    fireEvent.click(screen.getByTestId("ed-tr-restore-0"));
    expect(last(p.onCommit)).toEqual(restoreGap(d, 0));
  });

  it("double-click edits: Enter saves, Esc discards, empty hides", () => {
    const d = doc();
    const p = mount(d);
    fireEvent.doubleClick(word("w2"));
    let input = screen.getByTestId("ed-tr-edit") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "lieber" } });
    // Backspace while typing never cuts.
    fireEvent.keyDown(input, { key: "Backspace" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(p.onCommit).toHaveBeenCalledTimes(1);
    expect(last(p.onCommit)).toEqual(setWordText(d, null, "w2", "lieber"));

    fireEvent.doubleClick(word("w3"));
    input = screen.getByTestId("ed-tr-edit") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Erde" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(p.onCommit).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("ed-tr-edit")).toBeNull();

    fireEvent.doubleClick(word("w3"));
    input = screen.getByTestId("ed-tr-edit") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const hidden = last(p.onCommit).transcripts[0].words.find((w) => w.id === "w3");
    expect(hidden).toMatchObject({ text: "", originalText: "Welt" });
  });

  it("a hidden word stays visible struck through with what was heard", () => {
    const d = setWordText(doc(), null, "w3", "");
    mount(d);
    expect(word("w3").textContent).toBe("Welt");
    expect(word("w3").className).toContain("line-through");
  });

  it("the word being spoken is highlighted", () => {
    mount(doc(), { cue: { clip: 1, sourceMs: 6700 } });
    expect(word("w5").getAttribute("data-current")).toBe("true");
    expect(word("w1").getAttribute("data-current")).toBeNull();
    cleanup();
    // Same source time, other clip: not this word.
    mount(doc(), { cue: { clip: 0, sourceMs: 6700 } });
    expect(word("w5").getAttribute("data-current")).toBeNull();
  });

  it("without a transcript it offers Transcribe", () => {
    mount(doc({ transcripts: [] }));
    expect(screen.getByTestId("ed-tr-go")).toBeTruthy();
    expect(screen.queryByTestId("ed-transcript")).toBeNull();
  });
});
