import { describe, expect, it } from "vitest";
import type { TranscribeFile, TranscribeJob } from "./ipc";
import {
  fileStatus,
  formatDuration,
  isJobActive,
  jobCounts,
  overallProgress,
  phaseLabel,
  remainingMs,
} from "./transcribe";

function file(over: Partial<TranscribeFile> = {}): TranscribeFile {
  return {
    path: "/a/x.mp4",
    name: "x.mp4",
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

function job(files: TranscribeFile[], phase: TranscribeJob["phase"] = "running"): TranscribeJob {
  return { id: 1, phase, language: null, files, error: null, started_ms: 0, finished_ms: null };
}

describe("transcribe helpers", () => {
  it("knows which phases are still running", () => {
    expect(isJobActive(null)).toBe(false);
    for (const p of ["starting", "loading_model", "running"] as const) {
      expect(isJobActive(job([], p))).toBe(true);
    }
    for (const p of ["finished", "cancelled", "failed"] as const) {
      expect(isJobActive(job([], p))).toBe(false);
    }
  });

  it("overall progress counts settled files fully and the running one partly", () => {
    const j = job([
      file({ state: "done", progress: 1 }),
      file({ state: "failed" }),
      file({ state: "running", progress: 0.5 }),
      file(),
    ]);
    expect(overallProgress(j)).toBeCloseTo(2.5 / 4);
    expect(overallProgress(job([], "finished"))).toBe(1);
    expect(overallProgress(job([], "running"))).toBe(0);
  });

  it("estimates the rest only once it means something", () => {
    const running = (progress: number, started: number) =>
      file({ state: "running", progress, started_ms: started });
    expect(remainingMs(running(0.02, 0), 60_000)).toBeNull();
    expect(remainingMs(running(0.5, 0), 1_000)).toBeNull();
    expect(remainingMs(running(0.25, 0), 30_000)).toBeCloseTo(90_000);
    expect(remainingMs(file({ state: "done", progress: 1, started_ms: 0 }), 9)).toBeNull();
  });

  it("formats durations compactly", () => {
    expect(formatDuration(-5)).toBe("0 s");
    expect(formatDuration(42_000)).toBe("42 s");
    expect(formatDuration(185_000)).toBe("3:05 min");
    expect(formatDuration(3_720_000)).toBe("1:02 h");
  });

  it("describes every file state", () => {
    expect(fileStatus(file(), 0)).toBe("Wartet");
    expect(fileStatus(file({ state: "running", started_ms: 0 }), 10)).toBe("Audio wird gelesen…");
    expect(fileStatus(file({ state: "running", progress: 0.25, started_ms: 0 }), 30_000)).toBe(
      "25 % · noch ~1:30 min",
    );
    expect(
      fileStatus(file({ state: "done", progress: 1, language: "de", started_ms: 0, finished_ms: 42_000 }), 0),
    ).toBe("Fertig · de · 42 s");
    expect(fileStatus(file({ state: "failed", error: "Keine Tonspur" }), 0)).toBe(
      "Fehler: Keine Tonspur",
    );
    expect(fileStatus(file({ state: "cancelled" }), 0)).toBe("Abgebrochen");
  });

  it("names the job's phase and position in the batch", () => {
    const files = [file({ state: "done" }), file({ state: "running" }), file()];
    expect(phaseLabel(job(files))).toBe("Transkribiere 2 von 3");
    expect(phaseLabel(job([file({ state: "running" })]))).toBe("Transkribiere");
    expect(phaseLabel(job([], "loading_model"))).toBe("Modell wird geladen…");
    expect(phaseLabel(job([file({ state: "done" }), file({ state: "failed" })], "finished"))).toBe(
      "Fertig — 1 von 2 erstellt",
    );
    expect(phaseLabel(job([file({ state: "done" }), file({ state: "cancelled" })], "cancelled"))).toBe(
      "Abgebrochen — 1 von 2 fertig",
    );
    expect(jobCounts(job(files))).toEqual({ done: 1, failed: 0, cancelled: 0, total: 3 });
  });
});
