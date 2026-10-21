import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TranscribeLivePanel } from "./TranscribeLivePanel";
import type { LiveJob } from "../lib/ipc";

function job(over: Partial<LiveJob> = {}): LiveJob {
  return {
    id: 1, state: "recording", language: null, started_ms: 0, elapsed_ms: 65_000, level: 0.5,
    committed: "", partial: "", final_text: null, loading_model: null, copied: false, error: null,
    ...over,
  };
}
const noop = () => undefined;
const props = { startError: null, onStart: noop, onStop: noop, onCancel: noop, onCopy: noop };

describe("TranscribeLivePanel", () => {
  it("before a start it offers to start", () => {
    const onStart = vi.fn();
    render(<TranscribeLivePanel {...props} job={null} onStart={onStart} />);
    fireEvent.click(screen.getByText("Start recording"));
    expect(onStart).toHaveBeenCalled();
  });

  it("offers song mode next to dictation, and names the source while it runs", () => {
    const onStart = vi.fn();
    const { unmount } = render(<TranscribeLivePanel {...props} job={null} onStart={onStart} />);
    fireEvent.click(screen.getByText("Transcribe song"));
    expect(onStart).toHaveBeenCalledWith(true);
    unmount();
    render(<TranscribeLivePanel {...props} job={job({ source: "system" })} />);
    expect(screen.getByTestId("live-source").textContent).toContain("System audio");
  });

  it("while recording: clock, kept text, the open sentence greyed, stop button", () => {
    const onStop = vi.fn();
    render(
      <TranscribeLivePanel {...props} job={job({ committed: "Hallo Welt.", partial: "wie geht" })} onStop={onStop} />,
    );
    expect(screen.getByText("1:05")).toBeTruthy();
    expect(screen.getByText(/Hallo Welt\./)).toBeTruthy();
    expect(screen.getByTestId("live-partial").textContent).toContain("wie geht");
    fireEvent.click(screen.getByText("Stop recording"));
    expect(onStop).toHaveBeenCalled();
  });

  it("when done the accurate text replaces the live one and says it was copied", () => {
    const onCopy = vi.fn();
    render(
      <TranscribeLivePanel
        {...props}
        job={job({ state: "done", committed: "hallo welt", partial: "", final_text: "Hallo, Welt!", copied: true })}
        onCopy={onCopy}
      />,
    );
    expect(screen.getByText("Hallo, Welt!")).toBeTruthy();
    expect(screen.queryByText("hallo welt")).toBeNull();
    expect(screen.getByText("Copied to the clipboard")).toBeTruthy();
    expect(screen.queryByText("Stop recording")).toBeNull();
    fireEvent.click(screen.getByText("Copy"));
    expect(onCopy).toHaveBeenCalledWith("Hallo, Welt!");
  });

  it("a failed dictation still shows the words that were spoken", () => {
    render(
      <TranscribeLivePanel {...props} job={job({ state: "failed", committed: "Das bleibt.", error: "Whisper weg" })} />,
    );
    expect(screen.getByText("Das bleibt.")).toBeTruthy();
    expect(screen.getByText("Whisper weg")).toBeTruthy();
  });

  it("shows why a start failed", () => {
    render(<TranscribeLivePanel {...props} job={null} startError="kein Mikrofon" />);
    expect(screen.getByText("kein Mikrofon")).toBeTruthy();
  });
});
