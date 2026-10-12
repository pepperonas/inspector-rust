import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { TranscribeFile, TranscribeJob } from "../lib/ipc";

const ipc = vi.hoisted(() => ({ cancelled: 0, revealed: [] as string[] }));
vi.mock("../lib/ipc", () => ({
  transcribeCancel: vi.fn(async () => {
    ipc.cancelled++;
    return true;
  }),
  revealPath: vi.fn(async (p: string) => {
    ipc.revealed.push(p);
  }),
}));

import { TranscribePanel } from "./TranscribePanel";
import de from "../locales/de.json";
import en from "../locales/en.json";
import { setCatalog } from "../lib/i18n";

// These pins were written against the German wording; run them in German.
beforeEach(() => setCatalog("de", de));
afterEach(() => setCatalog("en", en));

function file(name: string, over: Partial<TranscribeFile> = {}): TranscribeFile {
  return {
    path: `/a/${name}`,
    name,
    state: "queued",
    progress: 0,
    output: null,
    language: null,
    error: null,
    started_ms: null,
    finished_ms: null,
    ...over,
  };
}

function job(files: TranscribeFile[], phase: TranscribeJob["phase"] = "running", id = 1): TranscribeJob {
  return { id, phase, language: null, files, error: null, started_ms: 0, finished_ms: null };
}

afterEach(() => {
  cleanup();
  ipc.cancelled = 0;
  ipc.revealed = [];
});

describe("TranscribePanel", () => {
  it("shows one row with its own progress per file", () => {
    render(
      <TranscribePanel
        job={job([
          file("eins.mp4", { state: "done", progress: 1, output: "/a/eins.txt" }),
          file("zwei.mp3", { state: "running", progress: 0.4, started_ms: Date.now() }),
          file("drei.wav"),
        ])}
        focused
        onExit={() => {}}
        onRestart={() => {}}
      />,
    );
    const rows = screen.getAllByTestId("transcribe-file");
    expect(rows.map((r) => r.dataset.state)).toEqual(["done", "running", "queued"]);
    expect(rows[1].textContent).toContain("40 %");
    expect(rows[2].textContent).toContain("Wartet");
    expect(screen.getByTestId("transcribe-phase").textContent).toBe("Transkribiere 2 von 3");
  });

  it("Esc only closes the view — the job is not cancelled", () => {
    const onExit = vi.fn();
    render(<TranscribePanel job={job([file("a.mp4", { state: "running" })])} focused onExit={onExit} onRestart={() => {}} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).toHaveBeenCalledTimes(1);
    expect(ipc.cancelled).toBe(0);
  });

  it("cancelling takes two clicks, and Esc disarms instead of closing", () => {
    const onExit = vi.fn();
    render(<TranscribePanel job={job([file("a.mp4", { state: "running" })])} focused onExit={onExit} onRestart={() => {}} />);
    fireEvent.click(screen.getByText("Abbrechen"));
    expect(ipc.cancelled).toBe(0);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Abbrechen"));
    fireEvent.click(screen.getByText("Wirklich abbrechen?"));
    expect(ipc.cancelled).toBe(1);
  });

  it("a finished job offers no cancel; Enter restarts and a done row reveals its transcript", () => {
    const onRestart = vi.fn();
    render(
      <TranscribePanel
        job={job([file("a.mp4", { state: "done", progress: 1, output: "/a/a.txt" })], "finished")}
        focused
        onExit={() => {}}
        onRestart={onRestart}
      />,
    );
    expect(screen.queryByText("Abbrechen")).toBeNull();
    fireEvent.keyDown(window, { key: "Enter" });
    expect(onRestart).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("transcribe-file"));
    expect(ipc.revealed).toEqual(["/a/a.txt"]);
  });

  it("Enter never restarts while a job runs", () => {
    const onRestart = vi.fn();
    render(<TranscribePanel job={job([file("a.mp4", { state: "running" })])} focused onExit={() => {}} onRestart={onRestart} />);
    fireEvent.keyDown(window, { key: "Enter" });
    expect(onRestart).not.toHaveBeenCalled();
  });

  it("shows why a job failed before it started", () => {
    render(
      <TranscribePanel
        job={{ ...job([], "failed"), error: "Keine Audio-/Videodatei im Finder ausgewählt" }}
        focused
        onExit={() => {}}
        onRestart={() => {}}
      />,
    );
    expect(screen.getByText("Keine Audio-/Videodatei im Finder ausgewählt")).toBeTruthy();
    expect(screen.getByTestId("transcribe-phase").textContent).toBe("Fehlgeschlagen");
  });
});
