import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { LiveJob, TranscribeFile, TranscribeJob } from "./ipc";
import {
  fileStatus,
  formatDuration,
  isJobActive,
  jobCounts,
  overallProgress,
  phaseLabel,
  remainingMs,
  liveAction,
  liveLanguage,
  isLiveActive,
  isLiveRecording,
  formatClock,
  liveRowLabel,
  liveResultText,
  modeOfSource,
  startArg,
} from "./transcribe";
import de from "../locales/de.json";
import en from "../locales/en.json";
import { setCatalog } from "./i18n";

// These pins were written against the German wording; run them in German.
beforeEach(() => setCatalog("de", de));
afterEach(() => setCatalog("en", en));

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

function live(over: Partial<LiveJob> = {}): LiveJob {
  return {
    id: 1, state: "recording", language: null, started_ms: 0, elapsed_ms: 0, level: 0,
    committed: "", partial: "", final_text: null, loading_model: null, copied: false, error: null,
    ...over,
  };
}

describe("live dictation helpers", () => {
  it("reads --start / --stop like the Rust parser", () => {
    expect(liveAction("--start")).toBe("start");
    expect(liveAction("--start --language de")).toBe("start");
    expect(liveAction("  --stop ")).toBe("stop");
    expect(liveAction("--song")).toBe("start");
    expect(liveAction("--start --lyrics -l en")).toBe("start");
    expect(liveAction("--stop --song")).toBe("stop");
    expect(liveAction("--room")).toBe("start");
    expect(liveAction("--raum -l de")).toBe("start");
    expect(liveAction("")).toBeNull();
    expect(liveAction("~/a.mp3 --language de")).toBeNull();
    // A word that merely contains the flag is a file name, not the flag.
    expect(liveAction("my--start.mp3")).toBeNull();
  });

  it("finds the language in every spelling", () => {
    expect(liveLanguage("--start --language de")).toBe("de");
    expect(liveLanguage("-l en --start")).toBe("en");
    expect(liveLanguage("--start --language=fr")).toBe("fr");
    expect(liveLanguage("--start")).toBeNull();
  });

  it("Enter means stop while listening, not while finalizing", () => {
    expect(isLiveRecording(live({ state: "starting" }))).toBe(true);
    expect(isLiveRecording(live({ state: "recording" }))).toBe(true);
    expect(isLiveRecording(live({ state: "finalizing" }))).toBe(false);
    expect(isLiveActive(live({ state: "finalizing" }))).toBe(true);
    expect(isLiveActive(live({ state: "done" }))).toBe(false);
    expect(isLiveActive(null)).toBe(false);
  });

  it("the row names what Enter does", () => {
    expect(liveRowLabel(null, "--start")).toBe("Diktat starten");
    expect(liveRowLabel(null, "--start -l de")).toBe("Diktat starten (de)");
    expect(liveRowLabel(live(), "--stop")).toBe("Diktat beenden");
    // Even if the line still says --start, a running dictation is stopped.
    expect(liveRowLabel(live(), "--start")).toBe("Diktat beenden");
    expect(liveRowLabel(live({ state: "finalizing" }), "--stop")).toBe("Endfassung wird erstellt…");
  });

  it("formats the recording clock", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(7_900)).toBe("0:07");
    expect(formatClock(750_000)).toBe("12:30");
    expect(formatClock(3_723_000)).toBe("1:02:03");
    expect(formatClock(-5)).toBe("0:00");
  });

  it("the accurate text wins, the live text is the fallback", () => {
    expect(liveResultText(live({ committed: "a b", partial: "c", final_text: "A, b c." }))).toBe("A, b c.");
    expect(liveResultText(live({ committed: "a b", partial: "c", final_text: "  " }))).toBe("a b c");
    expect(liveResultText(live({ committed: "", partial: "" }))).toBe("");
    // The session's end text (catalogue lyrics) wins over what was heard.
    expect(liveResultText(live({ committed: "heard", final_text: "Heard.", result: "T – A\n\nlyrics" }))).toBe("T – A\n\nlyrics");
    expect(liveResultText(live({ committed: "heard", result: "  " }))).toBe("heard");
  });
});

describe("live dictation in App (source pins)", () => {
  async function app(): Promise<string> {
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    return readFileSync(cwd + "/src/App.tsx", "utf8").replace(/\/\/.*$/gm, "");
  }

  it("Enter on a running dictation stops it — before the argument is even read", async () => {
    const code = await app();
    const branch = code.indexOf('commandKind === "transcribe" && (liveAction(arg) || isLiveActive(liveJobRef.current))');
    expect(branch).toBeGreaterThan(-1);
    const body = code.slice(branch, code.indexOf("} else if", branch + 10));
    const stop = body.indexOf("if (isLiveRecording(liveJobRef.current)) await stopDictation();");
    const start = body.indexOf("startDictation(arg)");
    expect(stop).toBeGreaterThan(-1);
    expect(start).toBeGreaterThan(stop);
  });

  it("a started dictation turns the command into transcribe --stop", async () => {
    const code = await app();
    const fn = code.slice(code.indexOf("const startDictation = useCallback"));
    expect(fn.slice(0, fn.indexOf("}, []);"))).toContain('setQuery("transcribe --stop")');
  });
});

describe("start modes", () => {
  it("maps each mode to its flags and back from the source", () => {
    expect(startArg("dictate")).toBe("--start");
    expect(startArg("song")).toBe("--start --song");
    expect(startArg("room")).toBe("--start --room");
    expect(modeOfSource("system")).toBe("song");
    expect(modeOfSource("room")).toBe("room");
    expect(modeOfSource("mic")).toBe("dictate");
    expect(modeOfSource(undefined)).toBe("dictate");
    for (const m of ["dictate", "song", "room"] as const) expect(modeOfSource(m === "song" ? "system" : m === "room" ? "room" : "mic")).toBe(m);
  });
});
