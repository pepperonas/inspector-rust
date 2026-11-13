// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAutocutResult, RecordEditDoc } from "../../lib/ipc";
import { cutMany } from "../../lib/autocut";
import { CleanupBox } from "./CleanupBox";

afterEach(cleanup);

const doc = (): RecordEditDoc =>
  ({
    version: 7,
    clips: [{ startMs: 0, endMs: 20_000, source: null }],
    speed: [],
    crop: [null],
    zooms: [],
    annotations: [],
    audioTracks: [],
    layoutRegions: [],
    effects: [],
    transcripts: [],
  }) as unknown as RecordEditDoc;

const found: RecordAutocutResult = {
  suggestions: [
    { id: "cut-0", kind: "silence", clip: 0, startMs: 2_000, endMs: 4_000, programmeMs: 2_000, text: "" },
    { id: "cut-1", kind: "filler", clip: 0, startMs: 6_000, endMs: 6_400, programmeMs: 6_000, text: "ähm" },
    { id: "cut-2", kind: "silence", clip: 0, startMs: 10_000, endMs: 11_000, programmeMs: 10_000, text: "" },
  ],
  hasMic: true,
  micPending: false,
  hasTranscript: true,
};

async function search(res: RecordAutocutResult = found, d = doc()) {
  const suggest = vi.fn(async () => res);
  const onCommit = vi.fn();
  const onSeek = vi.fn();
  const view = render(<CleanupBox doc={d} suggest={suggest} onCommit={onCommit} onSeek={onSeek} />);
  await act(async () => {
    fireEvent.click(screen.getByTestId("ed-cleanup-find"));
  });
  return { suggest, onCommit, onSeek, d, view };
}

describe("CleanupBox", () => {
  it("cuts nothing by searching", async () => {
    const { suggest, onCommit } = await search();
    expect(suggest).toHaveBeenCalledWith({ silences: true, fillers: true, minGapMs: 700 });
    expect(screen.getByTestId("ed-cleanup-result").textContent).toContain("ähm");
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("removes the ticked suggestions as one edit", async () => {
    const { onCommit, d } = await search();
    fireEvent.click(screen.getByTestId("ed-cleanup-check-cut-1"));
    fireEvent.click(screen.getByTestId("ed-cleanup-apply"));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit.mock.calls[0][0]).toEqual(cutMany(d, [found.suggestions[0], found.suggestions[2]]));
    // The list described the old document: it is gone.
    expect(screen.queryByTestId("ed-cleanup-result")).toBeNull();
  });

  it("drops the list when the document changed elsewhere", async () => {
    const { view, suggest, onCommit, onSeek } = await search();
    const other = { ...doc(), micGainDb: 3 } as RecordEditDoc;
    view.rerender(<CleanupBox doc={other} suggest={suggest} onCommit={onCommit} onSeek={onSeek} />);
    expect(screen.queryByTestId("ed-cleanup-result")).toBeNull();
  });

  it("jumps to a suggestion and can untick everything", async () => {
    const { onSeek, onCommit } = await search();
    fireEvent.click(screen.getAllByTitle("Jump there")[1]);
    expect(onSeek).toHaveBeenCalledWith(6_000);
    fireEvent.click(screen.getByText("Select none"));
    expect((screen.getByTestId("ed-cleanup-apply") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByTestId("ed-cleanup-apply"));
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("says what it could not look at", async () => {
    await search({ suggestions: [], hasMic: false, micPending: false, hasTranscript: false });
    const text = screen.getByTestId("ed-cleanup-result").textContent ?? "";
    expect(text).toContain("this recording has none");
    expect(text).toContain("Transcribe the recording first");
    expect(text).toContain("Nothing found.");
  });

  it("passes the chosen options on", async () => {
    const suggest = vi.fn(async () => found);
    render(<CleanupBox doc={doc()} suggest={suggest} onCommit={vi.fn()} onSeek={vi.fn()} />);
    fireEvent.click(screen.getByLabelText("Find filler words"));
    fireEvent.click(screen.getByText("2.0 s"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("ed-cleanup-find"));
    });
    expect(suggest).toHaveBeenCalledWith({ silences: true, fillers: false, minGapMs: 2000 });
  });
});
