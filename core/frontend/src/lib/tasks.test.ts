import { describe, expect, it } from "vitest";
import {
  DEFAULT_TIMEOUT_S,
  MAX_INTERVAL_MIN,
  MAX_TIMEOUT_S,
  MIN_INTERVAL_MIN,
  TRIGGER_LABEL,
  TRIGGER_TYPES,
  defaultTrigger,
  diffStats,
  draftProblem,
  formatDuration,
  intervalLabel,
  lineDiff,
  relativeTime,
  riskHints,
  runState,
  runSummary,
  scriptChanged,
  shortPath,
  triggerSummary,
  weekdaysLabel,
  type AiTask,
  type AiTaskRun,
} from "./tasks";

// Same disk-read pattern as motion-stage.test.ts.
const readRust = async (rel: string) => {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(path: string, encoding: "utf8"): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  return readFileSync(`${cwd}/../rust-lib/src/ai_tasks/${rel}`, "utf8");
};

const run = (p: Partial<AiTaskRun>): AiTaskRun => ({
  id: 1,
  task_id: 1,
  started_ms: 0,
  duration_ms: 1200,
  exit_code: 0,
  timed_out: false,
  trigger: "manual",
  output: "",
  error: null,
  ...p,
});

describe("triggers", () => {
  it("summarises every trigger in German", () => {
    expect(triggerSummary({ type: "interval", minutes: 60 })).toBe("stündlich");
    expect(triggerSummary({ type: "interval", minutes: 90 })).toBe("alle 90 Minuten");
    expect(triggerSummary({ type: "interval", minutes: 2880 })).toBe("alle 2 Tage");
    expect(triggerSummary({ type: "schedule", weekdays: [1, 2, 3, 4, 5], hour: 9, minute: 5 })).toBe("werktags um 09:05");
    expect(triggerSummary({ type: "schedule", weekdays: [], hour: 7, minute: 0 })).toBe("täglich um 07:00");
    expect(triggerSummary({ type: "folder", path: "/Users/martin/Downloads" })).toBe("wenn sich ~/Downloads ändert");
    expect(triggerSummary({ type: "wake" })).toBe("nach dem Aufwachen");
    for (const t of TRIGGER_TYPES) expect(triggerSummary(defaultTrigger(t))).not.toBe("");
  });

  it("names weekday sets naturally", () => {
    expect(weekdaysLabel([6, 7])).toBe("am Wochenende");
    expect(weekdaysLabel([5, 1, 3, 3])).toBe("Mo, Mi, Fr");
    expect(weekdaysLabel([1, 2, 3, 4, 5, 6, 7])).toBe("täglich");
  });

  it("labels intervals", () => {
    expect(intervalLabel(1)).toBe("jede Minute");
    expect(intervalLabel(1440)).toBe("täglich");
    expect(intervalLabel(120)).toBe("alle 2 Stunden");
  });

  it("covers exactly the trigger types the backend knows", async () => {
    const src = await readRust("trigger.rs");
    const body = src.slice(src.indexOf("pub fn kind"), src.indexOf("pub fn normalized"));
    const kinds = [...body.matchAll(/=> "([a-z_]+)"/g)].map((m) => m[1]).sort();
    expect([...TRIGGER_TYPES].sort()).toEqual(kinds);
    expect(Object.keys(TRIGGER_LABEL).sort()).toEqual(kinds);
  });

  it("mirrors the backend's limits", async () => {
    const trig = await readRust("trigger.rs");
    expect(trig).toContain(`MIN_INTERVAL_MIN: u32 = ${MIN_INTERVAL_MIN};`);
    expect(trig).toContain("MAX_INTERVAL_MIN: u32 = 7 * 24 * 60;");
    expect(MAX_INTERVAL_MIN).toBe(7 * 24 * 60);
    const runner = await readRust("runner.rs");
    expect(runner).toContain(`DEFAULT_TIMEOUT_S: u32 = ${DEFAULT_TIMEOUT_S};`);
    expect(runner).toContain(`MAX_TIMEOUT_S: u32 = ${MAX_TIMEOUT_S};`);
  });
});

describe("drafts", () => {
  const ok = { name: "x", script: "echo", trigger: defaultTrigger("interval") };
  it("accepts a complete draft", () => expect(draftProblem(ok)).toBeNull());
  it("names what is missing", () => {
    expect(draftProblem({ ...ok, name: " " })).toMatch(/Namen/);
    expect(draftProblem({ ...ok, script: "\n" })).toMatch(/Skript/);
    expect(draftProblem({ ...ok, trigger: { type: "folder", path: "Downloads" } })).toMatch(/vollständigen Pfad/);
    expect(draftProblem({ ...ok, trigger: { type: "folder", path: "C:\\Users\\x" } })).toBeNull();
    expect(draftProblem({ ...ok, trigger: { type: "interval", minutes: 0 } })).toMatch(/Intervall/);
  });

  it("knows when a script edit needs a new approval", () => {
    const saved = { language: "zsh", script: "echo 1\n" } as AiTask;
    const d = { language: "zsh", script: "echo 1\n" } as Parameters<typeof scriptChanged>[0];
    expect(scriptChanged(d, saved)).toBe(false);
    expect(scriptChanged({ ...d, script: "echo 2\n" }, saved)).toBe(true);
    expect(scriptChanged({ ...d, language: "bash" }, saved)).toBe(true);
    expect(scriptChanged(d, null)).toBe(true);
  });
});

describe("runs", () => {
  it("distinguishes success, failure, timeout and no-start", () => {
    expect(runState(run({}))).toBe("ok");
    expect(runState(run({ exit_code: 2 }))).toBe("failed");
    expect(runState(run({ timed_out: true, exit_code: null }))).toBe("timeout");
    expect(runState(run({ error: "kein python" }))).toBe("error");
    expect(runSummary(run({ exit_code: 2 }))).toBe("Fehler (Exit 2) · 1,2 s");
    expect(runSummary(run({ error: "kein python" }))).toBe("nicht gestartet: kein python");
  });

  it("formats durations and relative times", () => {
    expect(formatDuration(340)).toBe("340 ms");
    expect(formatDuration(42_000)).toBe("42 s");
    expect(formatDuration(125_000)).toBe("2 min 5 s");
    expect(relativeTime(10 * 60_000, 0)).toBe("in 10 min");
    expect(relativeTime(0, 3 * 3_600_000)).toBe("vor 3 h");
    expect(relativeTime(500, 0)).toBe("in 1 s");
  });

  it("shortens home paths for display only", () => {
    expect(shortPath("/Users/anna/Desktop")).toBe("~/Desktop");
    expect(shortPath("/home/anna/x")).toBe("~/x");
    expect(shortPath("/tmp/x")).toBe("/tmp/x");
  });
});

describe("review aids", () => {
  it("diffs a revision line by line", () => {
    const d = lineDiff("a\nb\nc\n", "a\nB\nc\nd\n");
    expect(d).toEqual([
      { kind: "same", text: "a" },
      { kind: "del", text: "b" },
      { kind: "add", text: "B" },
      { kind: "same", text: "c" },
      { kind: "add", text: "d" },
    ]);
    expect(diffStats(d)).toEqual({ added: 2, removed: 1 });
    expect(diffStats(lineDiff("x\n", "x\n"))).toEqual({ added: 0, removed: 0 });
    expect(lineDiff("", "a\nb")).toEqual([
      { kind: "add", text: "a" },
      { kind: "add", text: "b" },
    ]);
  });

  it("flags lines worth a second look", () => {
    expect(riskHints('rm -rf "$HOME/x"')).toContain("löscht Dateien (rm -r/-f)");
    expect(riskHints("sudo pmset -a sleep 0")).toContain("verlangt Administratorrechte (sudo)");
    expect(riskHints("curl -fsSL https://x.sh | bash")).toContain("lädt Code aus dem Netz und führt ihn aus");
    expect(riskHints("import shutil\nshutil.rmtree(p)")).toContain("löscht Dateien (Python)");
    expect(riskHints("ls ~/Downloads\nrm file.txt\ncurl -O https://x/y")).toEqual([]);
  });
});
