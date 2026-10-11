import { describe, expect, it } from "vitest";
import { COMMANDS, parseCommand } from "./commands";
import { ARG_TREES, completeArg, cycleIndex, ghostSuffix, tabStep } from "./arg-complete";

/** Complete like App does: parse the line, then ask the registry. */
function complete(line: string) {
  const p = parseCommand(line);
  if (!p) return null;
  return completeArg(p.spec.kind, line);
}

function values(line: string): string[] {
  return complete(line)?.candidates.map((c) => c.value) ?? [];
}

describe("completeArg", () => {
  it("completes the first argument word by prefix", () => {
    expect(values("spotify hi")).toEqual(["history"]);
    expect(values("spotify p")).toEqual(["play", "pause", "previous"]);
    expect(complete("spotify hi")?.candidates[0].completion).toBe("spotify history");
    expect(complete("spotify hi")?.partial).toBe("hi");
  });

  it("offers everything after a space, nothing before one", () => {
    expect(values("spotify ")[0]).toBe("now");
    expect(values("spotify ").length).toBe(9);
    // The bare keyword is the keyword's own business.
    expect(complete("spotify")?.candidates.length).toBe(9);
  });

  it("walks the tree: nested words, self-reference, the end of the tree", () => {
    expect(values("spotify history ")).toContain("woche");
    expect(values("spotify history heute ex")).toEqual(["export"]);
    expect(values("spotify history export ")).toEqual(["json", "csv", "md"]);
    expect(complete("spotify history export csv ")).toBeNull();
    // Free text ends it: nothing to complete after an artist name.
    expect(complete("spotify history artist Kanye ")).toBeNull();
    // An unknown word before the cursor ends it too.
    expect(complete("spotify dance pl")).toBeNull();
  });

  it("is case- and accent-insensitive but keeps the canonical spelling", () => {
    expect(complete("spotify HI")?.candidates[0].completion).toBe("spotify history");
  });

  it("returns nothing for a word already typed in full", () => {
    expect(complete("spotify history")).toBeNull();
    expect(complete("wakelock off")).toBeNull();
    // …unless a longer option shares the prefix.
    expect(values("wakelock o")).toEqual(["on", "off"]);
  });

  it("keeps the alias the user typed", () => {
    expect(complete("caffeine d")?.candidates[0].completion).toBe("caffeine dark");
    expect(complete("config spo")?.candidates[0].completion).toBe("config spotify");
  });

  it("draws dynamic lists from their registries", () => {
    expect(values("settings ").length).toBeGreaterThan(20);
    expect(values("8bit ")).toContain("on");
    expect(values("8bit ").length).toBeGreaterThan(4);
    expect(values("transcribe --language ")).toContain("de");
  });

  it("has no tree for commands that complete through their own rows", () => {
    for (const kind of ["weather", "convert", "figlet"] as const) {
      expect(ARG_TREES[kind]).toBeUndefined();
    }
  });

  it("only keys real command kinds", () => {
    const kinds = new Set(COMMANDS.map((c) => c.kind));
    for (const k of Object.keys(ARG_TREES)) expect(kinds.has(k as never)).toBe(true);
  });
});

describe("ghost + cycling", () => {
  it("shows the rest of the word only when the typed part matches exactly", () => {
    const c = complete("spotify hi")!.candidates[0];
    expect(ghostSuffix("spotify hi", c)).toBe("story");
    expect(ghostSuffix("spotify HI", complete("spotify HI")!.candidates[0])).toBe("");
    expect(ghostSuffix("spotify ", complete("spotify ")!.candidates[0])).toBe("now");
  });

  it("cycles forward and back with wrap-around", () => {
    expect(cycleIndex(0, 3, false)).toBe(1);
    expect(cycleIndex(2, 3, false)).toBe(0);
    expect(cycleIndex(0, 3, true)).toBe(2);
    expect(cycleIndex(0, 0, false)).toBe(0);
  });
});

describe("tabStep", () => {
  it("takes the first candidate, then cycles through the rest, Shift goes back", () => {
    const c = complete("spotify p")!;
    const first = tabStep(null, c, false)!;
    expect(first.cands[first.index].completion).toBe("spotify play");
    const second = tabStep(first, null, false)!;
    expect(second.cands[second.index].completion).toBe("spotify pause");
    const back = tabStep(second, null, true)!;
    expect(back.cands[back.index].completion).toBe("spotify play");
    expect(tabStep(back, null, true)!.index).toBe(2);
  });

  it("Shift+Tab without a cycle starts at the last candidate", () => {
    const s = tabStep(null, complete("spotify p"), true)!;
    expect(s.cands[s.index].value).toBe("previous");
  });

  it("does nothing without candidates", () => {
    expect(tabStep(null, null, false)).toBeNull();
  });
});

