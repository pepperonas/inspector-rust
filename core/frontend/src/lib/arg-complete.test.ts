import { describe, expect, it } from "vitest";
import { COMMANDS, lookupKeyword } from "./commands";
import {
  ARG_TREES,
  completeArg,
  cycleIndex,
  editDistance,
  ghostSuffix,
  missingSource,
  recordUse,
  SOURCE_SPOTIFY_ARTISTS,
  tabStep,
  typoDistance,
  usageKey,
  usageScore,
  USAGE_HALF_LIFE_MS,
  USAGE_KEEP,
  type ArgUsage,
  type CompleteContext,
} from "./arg-complete";

/** Complete like App does: parse the line, then ask the registry. */
function complete(line: string, ctx: CompleteContext = {}) {
  // Like App: the command comes from the first word (parseCommand is null
  // while a required argument is still missing — `wakelock `).
  const spec = lookupKeyword(line.trimStart().split(/\s/)[0] ?? "");
  if (!spec) return null;
  return completeArg(spec.kind, line, ctx);
}

function values(line: string, ctx: CompleteContext = {}): string[] {
  return complete(line, ctx)?.candidates.map((c) => c.value) ?? [];
}

const ARTISTS: CompleteContext = {
  sources: {
    [SOURCE_SPOTIFY_ARTISTS]: [
      { value: "Kanye West", hint: "12× gehört" },
      { value: "Kraftwerk", hint: "4× gehört" },
      { value: "Metallica", hint: "3× gehört" },
    ],
  },
};

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
    // Case doesn't block the ghost — Tab writes the proper spelling.
    expect(ghostSuffix("spotify HI", complete("spotify HI")!.candidates[0])).toBe("story");
    expect(ghostSuffix("spotify history artist kr", complete("spotify history artist kr", ARTISTS)!.candidates[0])).toBe("aftwerk");
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

describe("values", () => {
  it("suggests typical values after a word that takes one", () => {
    expect(values("spotify volume ")).toEqual(["25", "50", "75", "100"]);
    expect(values("spotify volume 7")).toEqual(["75"]);
    expect(values("timer ")).toContain("25min");
    expect(values("countdown 2")).toEqual(["25min"]);
    expect(values("shot ")).toEqual(["3", "5", "10"]);
    expect(values("pwgen 2")).toEqual(["24"]);
  });

  it("never 'corrects' a number — numbers are free", () => {
    expect(complete("timer 7min")).toBeNull();
    expect(complete("iris 60")).toBeNull();
  });
});

describe("artists from the history (live source)", () => {
  it("asks for the source until it is loaded", () => {
    expect(missingSource("spotify", "spotify history artist ")).toBe(SOURCE_SPOTIFY_ARTISTS);
    expect(missingSource("spotify", "spotify history artist ", ARTISTS)).toBeNull();
    expect(missingSource("spotify", "spotify history ")).toBeNull();
    expect(complete("spotify history artist ")).toBeNull();
  });

  it("completes names with spaces as one value", () => {
    expect(values("spotify history artist ", ARTISTS)).toEqual(["Kanye West", "Kraftwerk", "Metallica"]);
    expect(values("spotify history artist k", ARTISTS)).toEqual(["Kanye West", "Kraftwerk"]);
    const c = complete("spotify history artist kanye ", ARTISTS)!;
    expect(c.candidates.map((x) => x.value)).toEqual(["Kanye West"]);
    expect(c.candidates[0].completion).toBe("spotify history artist Kanye West");
    expect(complete("spotify history artist Kanye West", ARTISTS)).toBeNull();
    // Works after other filters too.
    expect(complete("spotify history woche artist met", ARTISTS)!.candidates[0].completion).toBe(
      "spotify history woche artist Metallica",
    );
  });
});

describe("typo correction", () => {
  it("measures one swapped pair as one mistake", () => {
    expect(editDistance("histroy", "history")).toBe(1);
    expect(editDistance("pasue", "pause")).toBe(1);
    expect(editDistance("abc", "abc")).toBe(0);
    expect(editDistance("", "abc")).toBe(3);
  });

  it("accepts slips in the whole word or its start, not wild guesses", () => {
    expect(typoDistance("histroy", "history")).toBe(1);
    expect(typoDistance("hsit", "history")).toBe(1);
    expect(typoDistance("hi", "history")).toBeNull(); // too short to guess
    expect(typoDistance("xyzzy", "history")).toBeNull();
    expect(typoDistance("histroyy", "history")).toBe(2); // long word: two allowed
  });

  it("offers the corrected word when nothing matches by prefix", () => {
    const c = complete("spotify histroy")!;
    expect(c.candidates[0]).toMatchObject({ value: "history", completion: "spotify history", fix: true });
    expect(ghostSuffix("spotify histroy", c.candidates[0])).toBe("");
    expect(values("wakelock dakr")).toEqual(["dark"]);
    // A prefix match is never treated as a typo.
    expect(complete("spotify hist")!.candidates[0].fix).toBeUndefined();
    // Nothing close: no suggestion at all.
    expect(complete("spotify qqqq")).toBeNull();
  });
});

describe("recently used first", () => {
  const NOW = 1_800_000_000_000;

  it("puts the habit first without dropping anything", () => {
    let u: ArgUsage = {};
    u = recordUse(u, "wakelock", "wakelock dark", {}, NOW);
    u = recordUse(u, "wakelock", "wakelock dark", {}, NOW);
    expect(values("wakelock ", { usage: u, now: NOW })).toEqual(["dark", "on", "off"]);
    expect(values("wakelock ")).toEqual(["on", "off", "dark"]);
  });

  it("records every level of the path, and live values only when real", () => {
    let u: ArgUsage = {};
    u = recordUse(u, "spotify", "spotify history woche artist Metallica", ARTISTS, NOW);
    expect(u[usageKey("spotify", [])].history.n).toBe(1);
    expect(u[usageKey("spotify", ["history"])].woche.n).toBe(1);
    expect(u[usageKey("spotify", ["history", "woche"])].artist.n).toBe(1);
    expect(u[usageKey("spotify", ["history", "woche", "artist"])].Metallica.n).toBe(1);
    // Free text is not a habit.
    const v = recordUse({}, "spotify", "spotify history artist Unbekannt", ARTISTS, NOW);
    expect(v[usageKey("spotify", ["history", "artist"])]).toBeUndefined();
    // Order follows usage at that position.
    expect(values("spotify history ", { usage: u, now: NOW })[0]).toBe("woche");
  });

  it("lets old habits fade", () => {
    expect(usageScore({ n: 4, last: NOW }, NOW)).toBe(4);
    expect(usageScore({ n: 4, last: NOW - USAGE_HALF_LIFE_MS }, NOW)).toBeCloseTo(2);
    let u: ArgUsage = {};
    for (let i = 0; i < 3; i++) u = recordUse(u, "wakelock", "wakelock on", {}, NOW - 10 * USAGE_HALF_LIFE_MS);
    u = recordUse(u, "wakelock", "wakelock off", {}, NOW);
    expect(values("wakelock ", { usage: u, now: NOW })[0]).toBe("off");
  });

  it("keeps a bounded number of values per position", () => {
    let u: ArgUsage = {};
    for (let i = 0; i < USAGE_KEEP + 10; i++) u = recordUse(u, "settings", `settings ${ARG_ids()[i]}`, {}, NOW + i);
    expect(Object.keys(u[usageKey("settings", [])]).length).toBe(USAGE_KEEP);
  });

  it("is a pure update — the input stays untouched", () => {
    const u: ArgUsage = {};
    recordUse(u, "wakelock", "wakelock on", {}, NOW);
    expect(u).toEqual({});
  });
});

function ARG_ids(): string[] {
  const node = ARG_TREES.settings;
  return (typeof node === "function" ? node() : (node as unknown as { value: string }[])).map((o) => o.value);
}

