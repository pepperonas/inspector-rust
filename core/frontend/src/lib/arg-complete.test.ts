import { describe, expect, it } from "vitest";
import { COMMANDS, lookupKeyword } from "./commands";
import {
  ARG_TREES,
  completeArg,
  editDistance,
  FREE_VALUE_ARG_KINDS,
  ghostSuffix,
  missingSource,
  recordUse,
  showArgSuggestions,
  spotifyRangeCycle,
  suggestionRows,
  tabOpens,
  SOURCE_SPOTIFY_ARTISTS,
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
    expect(values("spotify ").length).toBe(10);
    // The bare keyword is the keyword's own business.
    expect(complete("spotify")?.candidates.length).toBe(10);
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

  it("puts the dictation action that fits the state first, over any habit", () => {
    // Every dictation is a start AND a stop, so habit alone ranks them
    // arbitrarily — `--stop` used to win while nothing recorded.
    const habit: ArgUsage = { [usageKey("transcribe", [])]: { "--stop": { n: 9, last: 0 } } };
    const idle = complete("transcribe ", { usage: habit, now: 0, prefer: ["--start", "--stop"] });
    expect(idle?.candidates.map((c) => c.value).slice(0, 2)).toEqual(["--start", "--stop"]);
    const live = complete("transcribe ", { usage: habit, now: 0, prefer: ["--stop", "--start"] });
    expect(live?.candidates.map((c) => c.value).slice(0, 2)).toEqual(["--stop", "--start"]);
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

describe("Spotify command arguments", () => {
  it.each(["heute", "gestern", "woche", "monat", "today", "yesterday", "week", "month"])(
    "replaces previous intervals with %s, including legacy duplicates",
    (value) => {
      const c = complete("spotify history heute gestern gestern ")!.candidates.find((x) => x.value === value)!;
      expect(c.completion).toBe(`spotify history ${value}`);
      expect(c.hasNext).toBe(false);
      expect(complete(c.completion)).toBeNull(); // Enter runs, no further fill
    },
  );

  it("keeps filter/export options and completes aliases in both languages", () => {
    expect(complete("spotify verlauf heute ye")?.candidates[0].completion).toBe("spotify verlauf yesterday");
    expect(complete("spotify h week mo")?.candidates[0].completion).toBe("spotify h monat");
    expect(complete("spotify history today ex")?.candidates[0].completion).toBe("spotify history today export");
    expect(values("spotify vol ")).toEqual(["25", "50", "75", "100"]);
  });

  it("keeps all intervals switchable after typing or accepting a complete range", () => {
    const cycle = spotifyRangeCycle("spotify history yesterday")!;
    expect(cycle.cands).toHaveLength(8);
    expect(cycle.cands[cycle.index].value).toBe("yesterday");
    // As rows: the current interval first, every other one below it.
    const rows = suggestionRows("spotify", "spotify history yesterday", complete("spotify history yesterday"), false);
    expect(rows.range).toBe(true);
    expect(rows.cands[0].value).toBe("yesterday");
    expect(rows.cands.map((c) => c.value)).toEqual(expect.arrayContaining(["today", "week", "month"]));
    expect(rows.cands).toHaveLength(8);
    expect(spotifyRangeCycle("spotify history artist today")).toBeNull();
    expect(spotifyRangeCycle("spotify history today ")).toBeNull();
    const artist = complete("spotify history week artist to", {
      sources: { [SOURCE_SPOTIFY_ARTISTS]: [{ value: "today", hint: "artist" }] },
    })!.candidates[0];
    expect(artist.completion).toBe("spotify history week artist today");
  });

  it.each(["play", "pause", "stop", "toggle", "next", "skip", "weiter", "previous", "prev", "back", "zurück", "volume 7", "vol 7", "lautstärke 7", "volume 0", "volume 100", "seek 90", "seek 1:30", "spulen 1:02:05"])(
    "keeps Enter on the complete control %s, even with preset suggestions",
    (arg) => expect(showArgSuggestions("spotify", `spotify ${arg}`)).toBe(false),
  );
  it("still offers incomplete controls and interval options", () => {
    expect(showArgSuggestions("spotify", "spotify p")).toBe(true);
    expect(showArgSuggestions("spotify", "spotify volume ")).toBe(true);
    expect(showArgSuggestions("spotify", "spotify history ")).toBe(true);
    expect(values("spotify history ")).toEqual(expect.arrayContaining(["today", "yesterday", "week", "month"]));
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
});

describe("rows in the list + Tab (v0.218.0)", () => {
  it("shows no rows for a bare keyword — its own row stays", () => {
    expect(suggestionRows("spotify", "spotify", complete("spotify"), false).cands).toEqual([]);
  });

  it("shows enumerated options as rows once a space follows the keyword", () => {
    const rows = suggestionRows("spotify", "spotify p", complete("spotify p"), false);
    expect(rows.cands.map((c) => c.value)).toEqual(expect.arrayContaining(["play", "pause", "previous"]));
  });

  it("hides free-value presets until Tab opens the menu", () => {
    const c = complete("iris 5");
    expect(suggestionRows("iris", "iris 5", c, false).cands).toEqual([]);
    expect(suggestionRows("iris", "iris 5", c, true).cands.map((x) => x.value)).toEqual(["55"]);
  });

  it("keeps a ready Spotify control runnable, but the menu still offers presets", () => {
    const c = complete("spotify volume 7");
    expect(suggestionRows("spotify", "spotify volume 7", c, false).cands).toEqual([]);
    expect(suggestionRows("spotify", "spotify volume 7", c, true).cands.map((x) => x.value)).toEqual(["75"]);
  });

  it("Tab on a bare keyword adds the space so the rows appear, without a menu", () => {
    expect(tabOpens("spotify", "spotify", complete("spotify"))).toEqual({ line: "spotify ", menu: false });
  });

  it("Tab on a free-value command opens the menu on the current line", () => {
    expect(tabOpens("pwgen", "pwgen", complete("pwgen"))).toEqual({ line: "pwgen ", menu: true });
    expect(tabOpens("iris", "iris 5", complete("iris 5"))).toEqual({ line: "iris 5", menu: true });
  });

  it("Tab does nothing without candidates", () => {
    expect(tabOpens("spotify", "spotify zzzz", null)).toBeNull();
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

describe("hasNext (whether a fill opens the next level)", () => {
  it("marks options that lead to further options", () => {
    const c = complete("spotify ")!;
    const byVal = (v: string) => c.candidates.find((x) => x.value === v)!;
    // `history` → the day filters; `now` is a leaf.
    expect(byVal("history").hasNext).toBe(true);
    expect(byVal("now").hasNext).toBe(false);
    // A leaf choice at a deeper level.
    const on = complete("wakelock ")!.candidates.find((x) => x.value === "on")!;
    expect(on.hasNext).toBe(false);
    // Nested: `history export` leads to the formats.
    const exp = complete("spotify history ")!.candidates.find((x) => x.value === "export")!;
    expect(exp.hasNext).toBe(true);
  });
});

describe("FREE_VALUE_ARG_KINDS", () => {
  it("covers exactly the convenience-preset (free numeric) commands", () => {
    // These keep the ghost/Tab hint but must NOT take over the list as rows
    // (Enter would hijack a custom number like `iris 5`).
    expect([...FREE_VALUE_ARG_KINDS].sort()).toEqual(
      ["iris", "pwgen", "random", "shot-full", "shot-region", "shot-window", "timer", "uuid"].sort(),
    );
    // The enumerated commands (rows are safe) are NOT in the set.
    for (const k of ["spotify", "wakelock", "github", "adb", "gestures", "settings"] as const) {
      expect(FREE_VALUE_ARG_KINDS.has(k)).toBe(false);
    }
    // Every listed kind really has an ARG_TREES entry.
    for (const k of FREE_VALUE_ARG_KINDS) expect(ARG_TREES[k]).toBeTruthy();
  });
});

function ARG_ids(): string[] {
  const node = ARG_TREES.settings;
  return (typeof node === "function" ? node() : (node as unknown as { value: string }[])).map((o) => o.value);
}

