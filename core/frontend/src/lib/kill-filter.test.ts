import { describe, expect, it } from "vitest";
import { filterKillProcesses, KILL_LIST_CAP } from "./kill-filter";

const procs = [
  {
    pid: 100,
    name: "Slack",
    memory_mb: 400,
    exe: "/Applications/Slack.app/Contents/MacOS/Slack",
  },
  {
    pid: 200,
    name: "InspectorRust",
    memory_mb: 80,
    exe: "/Applications/InspectorRust.app/Contents/MacOS/inspector-rust",
  },
  {
    pid: 300,
    name: "Google Chrome Helper",
    memory_mb: 200,
    exe: "/Applications/Google Chrome.app/Contents/Frameworks/…/Chrome_Helper",
  },
  {
    pid: 1234,
    name: "node",
    memory_mb: 50,
    exe: "/usr/local/bin/node",
  },
];

describe("filterKillProcesses", () => {
  it("returns the full list when the pattern is empty", () => {
    expect(filterKillProcesses(procs, "")).toEqual(procs);
    expect(filterKillProcesses(procs, "   ")).toEqual(procs);
  });

  it("matches a single substring against name", () => {
    expect(filterKillProcesses(procs, "inspector").map((p) => p.pid)).toEqual([
      200,
    ]);
  });

  it("matches against the exe path", () => {
    expect(filterKillProcesses(procs, "usr/local").map((p) => p.pid)).toEqual([
      1234,
    ]);
  });

  it("multi-word patterns require every token (so 'inspector rust' hits InspectorRust)", () => {
    expect(
      filterKillProcesses(procs, "inspector rust").map((p) => p.pid),
    ).toEqual([200]);
    expect(
      filterKillProcesses(procs, "google chrome").map((p) => p.pid),
    ).toEqual([300]);
  });

  it("is case-insensitive", () => {
    expect(filterKillProcesses(procs, "SLACK").map((p) => p.pid)).toEqual([
      100,
    ]);
  });

  it("treats an all-digits pattern as an exact PID and floats it to the top", () => {
    const out = filterKillProcesses(procs, "1234");
    expect(out.map((p) => p.pid)).toEqual([1234]);
    // A PID that also substring-matches a name still floats the exact hit.
    const withDup = [
      ...procs,
      { pid: 999, name: "1234-helper", memory_mb: 1, exe: "" },
    ];
    expect(filterKillProcesses(withDup, "1234").map((p) => p.pid)).toEqual([
      1234,
      999,
    ]);
  });

  it("returns an empty list when nothing matches", () => {
    expect(filterKillProcesses(procs, "zzzz-nope")).toEqual([]);
  });
});

describe("KILL_LIST_CAP", () => {
  it("is a positive cap used by the picker", () => {
    expect(KILL_LIST_CAP).toBeGreaterThan(0);
  });
});

describe("filterKillProcesses extras", () => {
  it("does not match when only some tokens hit", () => {
    expect(filterKillProcesses(procs, "inspector chrome")).toEqual([]);
  });

  it("matches hyphenated names via separate tokens", () => {
    expect(
      filterKillProcesses(procs, "inspector-rust").map((p) => p.pid),
    ).toEqual([200]);
  });

  it("trims surrounding whitespace before matching", () => {
    expect(filterKillProcesses(procs, "  slack  ").map((p) => p.pid)).toEqual([
      100,
    ]);
  });

  it("returns a shallow copy for empty pattern (not the same array)", () => {
    const out = filterKillProcesses(procs, "");
    expect(out).toEqual(procs);
    expect(out).not.toBe(procs);
  });

  it("PID pattern that matches nothing stays empty", () => {
    expect(filterKillProcesses(procs, "99999")).toEqual([]);
  });

  it("does not treat mixed alnum as a PID", () => {
    expect(filterKillProcesses(procs, "12x34")).toEqual([]);
  });
});
