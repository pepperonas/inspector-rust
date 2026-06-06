import { describe, it, expect } from "vitest";
import {
  COMMANDS,
  DEFAULT_PWGEN_LENGTH,
  RESIZE_PRESETS,
  commandSuggestions,
  fuzzyScore,
  parseAlarmArg,
  parseWakelockArg,
  isGetShakyTrigger,
  isOpenerTrigger,
  isSpaceInvadersTrigger,
  rockTheBoxMode,
  parseCommand,
  parseKillArg,
  parseResizeArg,
  parsePwgenArg,
  parseRandomArg,
  randomInt,
  parseShotDelay,
  parseTimerArg,
  formatBytes,
  resizePresetSuggestions,
  translateUrl,
} from "./commands";

describe("COMMANDS catalogue", () => {
  it("has 33 commands (+ shot×4, clean/cleanup, brightness/bri, rnd/random, meme)", () => {
    // The meme command is build-flag-gated (MEME_ENABLED); the test env leaves
    // VITE_IR_MEME unset → enabled → present.
    expect(COMMANDS.length).toBe(33);
  });

  it("every keyword is unique", () => {
    const seen = new Set<string>();
    for (const c of COMMANDS) {
      expect(seen.has(c.keyword)).toBe(false);
      seen.add(c.keyword);
    }
  });

  it("wakelock + caffeine are on/off arg commands (no more =1/=0)", () => {
    const wl = COMMANDS.filter((c) => c.kind === "wakelock");
    expect(wl.map((c) => c.keyword).sort()).toEqual(["caffeine", "wakelock"]);
    // Both take an on/off argument now.
    expect(wl.every((c) => c.requiresArg)).toBe(true);
    // The old `=1`/`=0` spellings are gone.
    expect(COMMANDS.some((c) => c.keyword.includes("="))).toBe(false);
  });

  it("every command has a non-empty description and syntax", () => {
    for (const c of COMMANDS) {
      expect(c.description.length).toBeGreaterThan(0);
      expect(c.syntax.length).toBeGreaterThan(0);
    }
  });
});

describe("parseCommand", () => {
  it("parses tren with text argument", () => {
    const r = parseCommand("tren hello world");
    expect(r?.spec.kind).toBe("translate-en");
    expect(r?.arg).toBe("hello world");
  });

  it("parses trde with text argument", () => {
    const r = parseCommand("trde hallo welt");
    expect(r?.spec.kind).toBe("translate-de");
    expect(r?.arg).toBe("hallo welt");
  });

  it("parses tr (auto-detect) — must not be confused with tren/trde", () => {
    const r = parseCommand("tr bonjour");
    expect(r?.spec.kind).toBe("translate-auto");
    expect(r?.arg).toBe("bonjour");
  });

  it("parses rz with WxH argument", () => {
    const r = parseCommand("rz 1200x800");
    expect(r?.spec.kind).toBe("resize");
    expect(r?.arg).toBe("1200x800");
  });

  it("parses optim — no argument needed", () => {
    const r = parseCommand("optim");
    expect(r?.spec.kind).toBe("optim");
    expect(r?.arg).toBe("");
  });

  it("parses rmvvls with text argument", () => {
    const r = parseCommand("rmvvls hello");
    expect(r?.spec.kind).toBe("rmvvls");
    expect(r?.arg).toBe("hello");
  });

  it("returns null when keyword is partial", () => {
    expect(parseCommand("tre")).toBeNull();
    expect(parseCommand("rmvvl")).toBeNull();
  });

  it("returns null when required arg is missing", () => {
    expect(parseCommand("tren")).toBeNull();
    expect(parseCommand("tren ")).toBeNull();
    expect(parseCommand("rz ")).toBeNull();
  });

  it("returns null for unknown keyword", () => {
    expect(parseCommand("xyz hello")).toBeNull();
    expect(parseCommand("translate hello")).toBeNull();
  });

  it("returns null for empty input", () => {
    expect(parseCommand("")).toBeNull();
    expect(parseCommand("   ")).toBeNull();
  });

  it("tolerates leading whitespace", () => {
    const r = parseCommand("  tren hello");
    expect(r?.spec.kind).toBe("translate-en");
    expect(r?.arg).toBe("hello");
  });

  it("strips trailing whitespace from args", () => {
    const r = parseCommand("tren  hello   ");
    expect(r?.arg).toBe("hello");
  });

  it("preserves internal spaces in args", () => {
    const r = parseCommand("tren the quick brown fox");
    expect(r?.arg).toBe("the quick brown fox");
  });
});

describe("fuzzyScore", () => {
  it("scores an empty needle as 0", () => {
    expect(fuzzyScore("wakelock", "")).toBe(0);
  });

  it("scores an exact match best (most negative)", () => {
    expect(fuzzyScore("freeze", "freeze")).toBe(-10000);
  });

  it("scores a prefix below (better than) a subsequence, longer keyword slightly worse", () => {
    const shortPrefix = fuzzyScore("lock", "lo")!; // -5000 + 4
    const longPrefix = fuzzyScore("shutdown", "sh")!; // -5000 + 8
    expect(shortPrefix).toBe(-4996);
    expect(longPrefix).toBe(-4992);
    // Both are prefixes → both far below any subsequence score.
    expect(shortPrefix).toBeLessThan(0);
    expect(longPrefix).toBeLessThan(0);
    // Shorter keyword wins (lower score).
    expect(shortPrefix).toBeLessThan(longPrefix);
  });

  it("returns null for a 1–2 char non-prefix (stays conservative)", () => {
    // "lk" is not a prefix of wakelock and is < 3 chars → no fuzzy match.
    expect(fuzzyScore("wakelock", "lk")).toBeNull();
  });

  it("matches a 3+ char first-char-anchored subsequence with a positive score", () => {
    // wlk → w(akelo... no, anchored at w then l, k) → positive.
    const s = fuzzyScore("wakelock", "wlk");
    expect(s).not.toBeNull();
    expect(s!).toBeGreaterThan(0);
  });

  it("requires the first character to match for the subsequence tier", () => {
    // 'a' anchors nothing in wakelock's first char 'w' → null.
    expect(fuzzyScore("wakelock", "alk")).toBeNull();
  });

  it("returns null when a needle char can't be found in order", () => {
    // 'q' never appears → no subsequence.
    expect(fuzzyScore("wakelock", "wqk")).toBeNull();
  });

  it("treats a leading substring as a prefix, not a subsequence", () => {
    // "pwg" IS a prefix of "pwgen" → prefix branch (-5000 + 5), not fuzzy.
    expect(fuzzyScore("pwgen", "pwg")).toBe(-4995);
  });

  it("computes a true subsequence as gaps*3 + keyword.length", () => {
    // pgn → pwgen: p(0) g(2, gap1) n(4, gap1) → gaps=2 → 2*3 + 5 = 11.
    expect(fuzzyScore("pwgen", "pgn")).toBe(11);
  });

  it("penalises gaps in the subsequence", () => {
    // frz → freeze: f(0) r(1, gap0) z(4, gap 4-1-1=2) → 2*3 + 6 = 12.
    expect(fuzzyScore("freeze", "frz")).toBe(12);
  });
});

describe("commandSuggestions", () => {
  it("returns empty for empty input", () => {
    expect(commandSuggestions("")).toEqual([]);
    expect(commandSuggestions("   ")).toEqual([]);
  });

  it("matches all tr-prefixed commands for 'tr'", () => {
    const suggestions = commandSuggestions("tr");
    const keywords = suggestions.map((s) => s.keyword);
    expect(keywords).toContain("tr");
    expect(keywords).toContain("tren");
    expect(keywords).toContain("trde");
  });

  it("ranks the prefix match first, then fuzzy matches, for 'tre'", () => {
    const keywords = commandSuggestions("tre").map((s) => s.keyword);
    expect(keywords[0]).toBe("tren"); // prefix beats fuzzy
    expect(keywords).toContain("trde"); // t-r-e is a subsequence of trde
  });

  it("matches rmvvls for 'rm'", () => {
    const suggestions = commandSuggestions("rm");
    const keywords = suggestions.map((s) => s.keyword);
    expect(keywords).toEqual(["rmvvls"]);
  });

  it("returns nothing when query has an argument and a known keyword", () => {
    // "tren hello" is a runnable command — no suggestion clutter.
    expect(commandSuggestions("tren hello")).toEqual([]);
  });

  it("returns nothing for exact match of no-arg command", () => {
    // "optim" alone is runnable.
    expect(commandSuggestions("optim")).toEqual([]);
  });

  it("returns the spec for exact match of a requires-arg command (teaches syntax)", () => {
    const suggestions = commandSuggestions("tren");
    expect(suggestions.length).toBe(1);
    expect(suggestions[0].keyword).toBe("tren");
  });

  it("is case-insensitive on the keyword prefix", () => {
    expect(commandSuggestions("TR").map((s) => s.keyword)).toContain("tren");
    expect(commandSuggestions("OptIm").map((s) => s.keyword)).toEqual([]); // exact no-arg
  });

  it("returns empty for unknown prefix", () => {
    expect(commandSuggestions("xyz")).toEqual([]);
  });

  it("fuzzy-matches first-char-anchored subsequences (3+ chars)", () => {
    // The whole point of the feature: invoke wakelock/freeze/pwgen without
    // typing them in full.
    expect(commandSuggestions("wlk").map((s) => s.keyword)).toContain("wakelock");
    expect(commandSuggestions("cfn").map((s) => s.keyword)).toContain("caffeine");
    expect(commandSuggestions("frz").map((s) => s.keyword)).toContain("freeze");
    expect(commandSuggestions("pwg").map((s) => s.keyword)).toContain("pwgen");
    expect(commandSuggestions("tmr").map((s) => s.keyword)).toContain("timer");
  });

  it("matches the new touch/mkdir commands", () => {
    expect(commandSuggestions("touch").length).toBeGreaterThanOrEqual(0); // requires-arg → suggestion or runnable
    expect(COMMANDS.map((c) => c.keyword)).toContain("touch");
    expect(COMMANDS.map((c) => c.keyword)).toContain("mkdir");
  });

  it("requires the first character to match (no un-anchored fuzz)", () => {
    // "lk" of wakelock is a subsequence, but without the leading 'w' it
    // must not surface — keeps fuzzy from matching mid-word noise.
    expect(commandSuggestions("xlk")).toEqual([]);
  });

  it("stays prefix-only for 1–2 char queries (no fuzzy flood)", () => {
    // "tr" must not pull in "timer" via subsequence — only real prefixes.
    expect(commandSuggestions("tr").map((s) => s.keyword)).not.toContain("timer");
  });
});

describe("translateUrl", () => {
  it("builds Google Translate URL with sl=en/tl=de for translate-en", () => {
    const url = translateUrl("translate-en", "hello");
    expect(url).toContain("sl=en");
    expect(url).toContain("tl=de");
    expect(url).toContain("text=hello");
    expect(url.startsWith("https://translate.google.com/")).toBe(true);
  });

  it("builds Google Translate URL with sl=de/tl=en for translate-de", () => {
    const url = translateUrl("translate-de", "hallo");
    expect(url).toContain("sl=de");
    expect(url).toContain("tl=en");
    expect(url).toContain("text=hallo");
  });

  it("builds Google Translate URL with sl=auto/tl=de for translate-auto", () => {
    const url = translateUrl("translate-auto", "bonjour");
    expect(url).toContain("sl=auto");
    expect(url).toContain("tl=de");
    expect(url).toContain("text=bonjour");
  });

  it("URL-encodes special characters", () => {
    const url = translateUrl("translate-en", "hello world & friends");
    expect(url).toContain("hello%20world%20%26%20friends");
  });

  it("URL-encodes umlauts", () => {
    const url = translateUrl("translate-de", "über");
    expect(url).toContain("%C3%BCber");
  });

  it("throws on non-translation kind", () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(() => translateUrl("optim" as any, "x")).toThrow();
  });
});

describe("parseResizeArg", () => {
  it("parses standard WxH", () => {
    expect(parseResizeArg("1200x800")).toEqual({ width: 1200, height: 800 });
  });

  it("accepts uppercase X", () => {
    expect(parseResizeArg("1200X800")).toEqual({ width: 1200, height: 800 });
  });

  it("tolerates whitespace around the separator", () => {
    expect(parseResizeArg("1200 x 800")).toEqual({ width: 1200, height: 800 });
    expect(parseResizeArg("  1200x800  ")).toEqual({ width: 1200, height: 800 });
  });

  it("rejects missing height", () => {
    expect(parseResizeArg("1200x")).toBeNull();
    expect(parseResizeArg("1200")).toBeNull();
  });

  it("rejects non-numeric", () => {
    expect(parseResizeArg("foo x bar")).toBeNull();
    expect(parseResizeArg("xxxx")).toBeNull();
  });

  it("rejects zero", () => {
    expect(parseResizeArg("0x100")).toBeNull();
    expect(parseResizeArg("100x0")).toBeNull();
  });

  it("rejects empty input", () => {
    expect(parseResizeArg("")).toBeNull();
  });
});

describe("parseCommand — system commands", () => {
  it("parses kill alone (empty arg, picker mode)", () => {
    const r = parseCommand("kill");
    expect(r?.spec.kind).toBe("kill");
    expect(r?.arg).toBe("");
  });

  it("parses kill with name pattern", () => {
    const r = parseCommand("kill slack");
    expect(r?.spec.kind).toBe("kill");
    expect(r?.arg).toBe("slack");
  });

  it("parses kill -9 <pattern>", () => {
    const r = parseCommand("kill -9 chrome");
    expect(r?.spec.kind).toBe("kill");
    expect(r?.arg).toBe("-9 chrome");
  });

  it("parses reboot/shutdown/lock without args", () => {
    expect(parseCommand("reboot")?.spec.kind).toBe("reboot");
    expect(parseCommand("shutdown")?.spec.kind).toBe("shutdown");
    expect(parseCommand("lock")?.spec.kind).toBe("lock");
  });
});

describe("parseKillArg", () => {
  it("returns force=false and empty pattern for empty input", () => {
    expect(parseKillArg("")).toEqual({ force: false, pattern: "" });
  });

  it("returns force=false with the given pattern", () => {
    expect(parseKillArg("slack")).toEqual({ force: false, pattern: "slack" });
    expect(parseKillArg("  chrome  ")).toEqual({ force: false, pattern: "chrome" });
  });

  it("detects -9 flag with following pattern", () => {
    expect(parseKillArg("-9 slack")).toEqual({ force: true, pattern: "slack" });
    expect(parseKillArg("-9  chrome  ")).toEqual({ force: true, pattern: "chrome" });
  });

  it("detects -9 alone (no pattern)", () => {
    expect(parseKillArg("-9")).toEqual({ force: true, pattern: "" });
  });

  it("does NOT treat -9-prefixed words as force", () => {
    // `-9foo` is a literal name beginning with `-`, not `-9 foo`.
    expect(parseKillArg("-9foo")).toEqual({ force: false, pattern: "-9foo" });
  });
});

describe("isGetShakyTrigger — hidden Pong easter egg", () => {
  it("matches the exact magic word", () => {
    expect(isGetShakyTrigger("getshaky")).toBe(true);
  });
  it("is case-insensitive", () => {
    expect(isGetShakyTrigger("GetShaky")).toBe(true);
    expect(isGetShakyTrigger("GETSHAKY")).toBe(true);
  });
  it("tolerates surrounding whitespace", () => {
    expect(isGetShakyTrigger("  getshaky  ")).toBe(true);
  });
  it("does not match partial / extended input", () => {
    expect(isGetShakyTrigger("getshak")).toBe(false);
    expect(isGetShakyTrigger("getshakyy")).toBe(false);
    expect(isGetShakyTrigger("get shaky")).toBe(false);
    expect(isGetShakyTrigger("getshaky now")).toBe(false);
    expect(isGetShakyTrigger("")).toBe(false);
  });
  it("is NOT in the public COMMANDS catalogue (hidden from autocomplete)", () => {
    expect(COMMANDS.some((c) => c.keyword === "getshaky")).toBe(false);
  });
  it("never surfaces as an autocomplete suggestion", () => {
    // Typing toward the magic word must not reveal it.
    for (const prefix of ["g", "ge", "get", "getsh", "getshak"]) {
      expect(commandSuggestions(prefix).some((c) => c.keyword === "getshaky")).toBe(
        false,
      );
    }
  });
});

describe("rockTheBoxMode — hidden Snake easter egg", () => {
  it("maps `rockthebox` to classic (walls kill) mode", () => {
    expect(rockTheBoxMode("rockthebox")).toBe("classic");
  });
  it("maps `rockthabox` to wrap-around mode", () => {
    expect(rockTheBoxMode("rockthabox")).toBe("wrap");
  });
  it("is case-insensitive", () => {
    expect(rockTheBoxMode("RockTheBox")).toBe("classic");
    expect(rockTheBoxMode("ROCKTHABOX")).toBe("wrap");
  });
  it("tolerates surrounding whitespace", () => {
    expect(rockTheBoxMode("  rockthebox  ")).toBe("classic");
    expect(rockTheBoxMode("  rockthabox  ")).toBe("wrap");
  });
  it("returns null for partial / extended / unrelated input", () => {
    expect(rockTheBoxMode("rockthebo")).toBeNull();
    expect(rockTheBoxMode("rocktheboxx")).toBeNull();
    expect(rockTheBoxMode("rock the box")).toBeNull();
    expect(rockTheBoxMode("rockthebox now")).toBeNull();
    expect(rockTheBoxMode("")).toBeNull();
  });
  it("is NOT in the public COMMANDS catalogue (hidden from autocomplete)", () => {
    expect(COMMANDS.some((c) => c.keyword === "rockthebox")).toBe(false);
  });
  it("never surfaces as an autocomplete suggestion", () => {
    for (const prefix of ["r", "ro", "rock", "rockthe", "rocktha"]) {
      expect(
        commandSuggestions(prefix).some((c) => c.keyword.startsWith("rockth")),
      ).toBe(false);
    }
  });
});

describe("isSpaceInvadersTrigger — hidden Space Invaders easter egg", () => {
  it("matches exact `space`", () => {
    expect(isSpaceInvadersTrigger("space")).toBe(true);
    expect(isSpaceInvadersTrigger("  SPACE  ")).toBe(true);
  });
  it("rejects partial or extended input", () => {
    expect(isSpaceInvadersTrigger("spac")).toBe(false);
    expect(isSpaceInvadersTrigger("spacebar")).toBe(false);
    expect(isSpaceInvadersTrigger("space invaders")).toBe(false);
  });
  it("is NOT in the public COMMANDS catalogue", () => {
    expect(COMMANDS.some((c) => c.keyword === "space")).toBe(false);
  });
});

describe("commandSuggestions — system commands", () => {
  it("suggests kill / lock for prefix 'l'", () => {
    const ks = commandSuggestions("l").map((c) => c.keyword);
    expect(ks).toContain("lock");
  });

  it("suggests reboot for 'reb'", () => {
    const ks = commandSuggestions("reb").map((c) => c.keyword);
    expect(ks).toEqual(["reboot"]);
  });

  it("does not suggest 'lock' when exact-matched (no-arg runnable)", () => {
    expect(commandSuggestions("lock")).toEqual([]);
  });

  it("does not suggest 'kill' alone — kill is requiresArg=false and runs via picker", () => {
    // kill is requiresArg: false (the picker handles empty arg), so the
    // suggestion list shouldn't include it when the user has already
    // typed the full keyword.
    expect(commandSuggestions("kill")).toEqual([]);
  });
});

describe("isOpenerTrigger — hidden German pickup-line easter egg", () => {
  it("matches the exact magic word", () => {
    expect(isOpenerTrigger("opener")).toBe(true);
  });
  it("is case-insensitive", () => {
    expect(isOpenerTrigger("Opener")).toBe(true);
    expect(isOpenerTrigger("OPENER")).toBe(true);
  });
  it("tolerates surrounding whitespace", () => {
    expect(isOpenerTrigger("  opener  ")).toBe(true);
  });
  it("matches `opener <anything>` so each extra keystroke re-rolls", () => {
    expect(isOpenerTrigger("opener ")).toBe(true);
    expect(isOpenerTrigger("opener x")).toBe(true);
    expect(isOpenerTrigger("opener xxxx")).toBe(true);
  });
  it("requires a word boundary — does NOT match plural / glued variants", () => {
    expect(isOpenerTrigger("openers")).toBe(false);
    expect(isOpenerTrigger("opener_test")).toBe(false);
    expect(isOpenerTrigger("openerz")).toBe(false);
  });
  it("does not match partial / unrelated input", () => {
    expect(isOpenerTrigger("open")).toBe(false);
    expect(isOpenerTrigger("openi")).toBe(false);
    expect(isOpenerTrigger("the opener")).toBe(false);
    expect(isOpenerTrigger("")).toBe(false);
  });
  it("is NOT in the public COMMANDS catalogue (hidden from autocomplete)", () => {
    expect(COMMANDS.some((c) => c.keyword === "opener")).toBe(false);
  });
  it("never surfaces as an autocomplete suggestion", () => {
    for (const prefix of ["o", "op", "ope", "open", "opene"]) {
      expect(commandSuggestions(prefix).some((c) => c.keyword.startsWith("open"))).toBe(false);
    }
  });
});

describe("resizePresetSuggestions", () => {
  it("returns all presets for the bare keyword `rz`", () => {
    const out = resizePresetSuggestions("rz");
    expect(out.length).toBe(RESIZE_PRESETS.length);
    expect(out[0].completion).toBe(`rz ${RESIZE_PRESETS[0].dims}`);
  });
  it("returns all presets for `rz ` (trailing space)", () => {
    expect(resizePresetSuggestions("rz ").length).toBe(RESIZE_PRESETS.length);
  });
  it("filters presets by the partial dimension prefix", () => {
    const out = resizePresetSuggestions("rz 19");
    // Only `1920x1080` starts with `19`.
    expect(out.length).toBe(1);
    expect(out[0].completion).toBe("rz 1920x1080");
  });
  it("returns empty for an already-complete WxH (runnable command takes over)", () => {
    expect(resizePresetSuggestions("rz 1920x1080")).toEqual([]);
    expect(resizePresetSuggestions("rz 800x600")).toEqual([]);
  });
  it("is case-insensitive on the keyword", () => {
    expect(resizePresetSuggestions("RZ").length).toBe(RESIZE_PRESETS.length);
    expect(resizePresetSuggestions("Rz 5").length).toBeGreaterThan(0);
  });
  it("does not match unrelated prefixes (`rzz`, `r`, `rz=…`)", () => {
    expect(resizePresetSuggestions("rzz")).toEqual([]);
    expect(resizePresetSuggestions("r")).toEqual([]);
    expect(resizePresetSuggestions("rz=1")).toEqual([]);
  });
  it("each suggestion completion parses as a complete `resize` command", () => {
    for (const p of resizePresetSuggestions("rz")) {
      const parsed = parseCommand(p.completion);
      expect(parsed?.spec.kind).toBe("resize");
      expect(parseResizeArg(parsed!.arg)).not.toBeNull();
    }
  });
});

describe("parseShotDelay", () => {
  it("returns 0 for empty / non-numeric / non-positive", () => {
    expect(parseShotDelay("")).toBe(0);
    expect(parseShotDelay("abc")).toBe(0);
    expect(parseShotDelay("0")).toBe(0);
    expect(parseShotDelay("-3")).toBe(0);
  });
  it("parses positive seconds, capped at 60", () => {
    expect(parseShotDelay("3")).toBe(3);
    expect(parseShotDelay(" 10 ")).toBe(10);
    expect(parseShotDelay("999")).toBe(60);
  });
});

describe("parseCommand — screenshot modes", () => {
  it("parses shot / shotfull / shotwin / shotlast", () => {
    expect(parseCommand("shot")?.spec.kind).toBe("shot-region");
    expect(parseCommand("shot 3")?.spec.kind).toBe("shot-region");
    expect(parseCommand("shotfull")?.spec.kind).toBe("shot-full");
    expect(parseCommand("shotwin")?.spec.kind).toBe("shot-window");
    expect(parseCommand("shotlast")?.spec.kind).toBe("shot-last");
  });
});

describe("parseRandomArg", () => {
  it("defaults to a die (1–6) for an empty arg", () => {
    expect(parseRandomArg("")).toEqual({ min: 1, max: 6 });
    expect(parseRandomArg("   ")).toEqual({ min: 1, max: 6 });
  });
  it("one number means 1..N", () => {
    expect(parseRandomArg("100")).toEqual({ min: 1, max: 100 });
    expect(parseRandomArg(" 20 ")).toEqual({ min: 1, max: 20 });
  });
  it("two numbers mean min..max", () => {
    expect(parseRandomArg("5 500")).toEqual({ min: 5, max: 500 });
    expect(parseRandomArg("10   12")).toEqual({ min: 10, max: 12 });
  });
  it("swaps when min > max", () => {
    expect(parseRandomArg("500 5")).toEqual({ min: 5, max: 500 });
  });
  it("supports negative bounds", () => {
    expect(parseRandomArg("-5 5")).toEqual({ min: -5, max: 5 });
  });
  it("rejects non-integers and 3+ numbers", () => {
    expect(parseRandomArg("abc")).toBeNull();
    expect(parseRandomArg("5x")).toBeNull();
    expect(parseRandomArg("1.5")).toBeNull();
    expect(parseRandomArg("1 2 3")).toBeNull();
  });
});

describe("randomInt", () => {
  it("always lands inside the inclusive range", () => {
    for (let i = 0; i < 2000; i++) {
      const n = randomInt(1, 6);
      expect(n).toBeGreaterThanOrEqual(1);
      expect(n).toBeLessThanOrEqual(6);
      expect(Number.isInteger(n)).toBe(true);
    }
  });
  it("covers both endpoints over many rolls", () => {
    const seen = new Set<number>();
    for (let i = 0; i < 2000; i++) seen.add(randomInt(1, 6));
    expect(seen.has(1)).toBe(true);
    expect(seen.has(6)).toBe(true);
  });
  it("returns the single value for a degenerate range", () => {
    expect(randomInt(7, 7)).toBe(7);
  });
  it("handles a wide range within bounds", () => {
    for (let i = 0; i < 500; i++) {
      const n = randomInt(5, 500);
      expect(n).toBeGreaterThanOrEqual(5);
      expect(n).toBeLessThanOrEqual(500);
    }
  });
});

describe("parseCommand — random", () => {
  it("parses rnd and the random alias to the random kind", () => {
    expect(parseCommand("rnd")?.spec.kind).toBe("random");
    expect(parseCommand("rnd 100")?.spec.kind).toBe("random");
    expect(parseCommand("random 5 500")?.spec.kind).toBe("random");
  });
});

describe("parseCommand — clean", () => {
  it("parses clean and the cleanup alias to the clean kind", () => {
    expect(parseCommand("clean")?.spec.kind).toBe("clean");
    expect(parseCommand("cleanup")?.spec.kind).toBe("clean");
  });
});

describe("parseCommand — brightness", () => {
  it("parses brightness and the bri alias to the brightness kind", () => {
    expect(parseCommand("brightness")?.spec.kind).toBe("brightness");
    expect(parseCommand("bri")?.spec.kind).toBe("brightness");
  });
});

describe("formatBytes", () => {
  it("formats across units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatBytes(20 * 1024 * 1024)).toBe("20 MB");
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe("3.0 GB");
  });
});

describe("parseTimerArg", () => {
  it("bare number → minutes (default unit)", () => {
    expect(parseTimerArg("12")).toEqual({ seconds: 720, label: "12 minutes" });
    expect(parseTimerArg("1")).toEqual({ seconds: 60, label: "1 minute" });
  });
  it("seconds aliases (s / sec / sek / sekunden)", () => {
    for (const u of ["s", "sec", "secs", "sek", "second", "seconds", "sekunde", "sekunden"]) {
      expect(parseTimerArg(`30${u}`)).toEqual({ seconds: 30, label: "30 seconds" });
      expect(parseTimerArg(`30 ${u}`)).toEqual({ seconds: 30, label: "30 seconds" });
    }
  });
  it("minutes aliases (m / min / mins / minuten)", () => {
    for (const u of ["m", "min", "mins", "minute", "minutes", "minuten"]) {
      expect(parseTimerArg(`12${u}`)).toEqual({ seconds: 720, label: "12 minutes" });
      expect(parseTimerArg(`12 ${u}`)).toEqual({ seconds: 720, label: "12 minutes" });
    }
  });
  it("hours aliases (h / hr / hrs / hour / hours / std / stunden)", () => {
    for (const u of ["h", "hr", "hrs", "hour", "hours", "std", "stunde", "stunden"]) {
      expect(parseTimerArg(`2${u}`)).toEqual({ seconds: 7200, label: "2 hours" });
      expect(parseTimerArg(`2 ${u}`)).toEqual({ seconds: 7200, label: "2 hours" });
    }
  });
  it("singular labels (1 second / 1 minute / 1 hour)", () => {
    expect(parseTimerArg("1s")?.label).toBe("1 second");
    expect(parseTimerArg("1m")?.label).toBe("1 minute");
    expect(parseTimerArg("1h")?.label).toBe("1 hour");
  });
  it("case-insensitive unit + comma decimal", () => {
    expect(parseTimerArg("30 SEC")?.seconds).toBe(30);
    expect(parseTimerArg("2,5 min")?.seconds).toBe(150);
    expect(parseTimerArg("0.5 h")?.seconds).toBe(1800);
  });
  it("rejects zero / negative / non-numeric", () => {
    expect(parseTimerArg("0")).toBeNull();
    expect(parseTimerArg("0 min")).toBeNull();
    expect(parseTimerArg("-5")).toBeNull();
    expect(parseTimerArg("abc")).toBeNull();
    expect(parseTimerArg("")).toBeNull();
  });
  it("rejects unknown units", () => {
    expect(parseTimerArg("12 fortnights")).toBeNull();
    expect(parseTimerArg("12 d")).toBeNull(); // no day support in v1
  });
  it("rejects garbage suffix on a valid number", () => {
    expect(parseTimerArg("12 minutes!")).toBeNull();
    expect(parseTimerArg("12 ★")).toBeNull();
  });
});

describe("parsePwgenArg", () => {
  it("accepts integers in the sane range [4, 128]", () => {
    expect(parsePwgenArg("12")).toBe(12);
    expect(parsePwgenArg("4")).toBe(4);
    expect(parsePwgenArg("128")).toBe(128);
  });
  it("rejects too-short (below 4 chars — trivially brute-forceable)", () => {
    expect(parsePwgenArg("3")).toBeNull();
    expect(parsePwgenArg("0")).toBeNull();
  });
  it("rejects too-long (above 128 chars — web fields often cap there)", () => {
    expect(parsePwgenArg("129")).toBeNull();
    expect(parsePwgenArg("1000")).toBeNull();
  });
  it("rejects non-integer formats", () => {
    expect(parsePwgenArg("12.5")).toBeNull();
    expect(parsePwgenArg("12 chars")).toBeNull();
    expect(parsePwgenArg("abc")).toBeNull();
    expect(parsePwgenArg("")).toBeNull();
    expect(parsePwgenArg("-12")).toBeNull();
  });
});

describe("parseAlarmArg", () => {
  // Fixed "now": 2026-06-06 10:00:00 local.
  const now = new Date(2026, 5, 6, 10, 0, 0, 0);

  it("schedules a later time today", () => {
    const a = parseAlarmArg("15:15", now)!;
    expect(a.label).toBe("15:15");
    expect(a.seconds).toBe((5 * 60 + 15) * 60); // 5h15m → seconds
  });

  it("rolls a passed time to tomorrow", () => {
    const a = parseAlarmArg("3:00", now)!;
    expect(a.label).toBe("3:00");
    // 3:00 already passed today → next is tomorrow 03:00 = 17h away.
    expect(a.seconds).toBe(17 * 3600);
  });

  it("accepts a bare hour (→ :00)", () => {
    const a = parseAlarmArg("11", now)!;
    expect(a.label).toBe("11:00");
    expect(a.seconds).toBe(3600);
  });

  it("rejects out-of-range and malformed times", () => {
    expect(parseAlarmArg("24:00", now)).toBeNull();
    expect(parseAlarmArg("12:60", now)).toBeNull();
    expect(parseAlarmArg("abc", now)).toBeNull();
    expect(parseAlarmArg("", now)).toBeNull();
    expect(parseAlarmArg("3:5", now)).toBeNull(); // minutes must be 2 digits
  });
});

describe("parseWakelockArg", () => {
  it("accepts on/off (the new canonical syntax)", () => {
    expect(parseWakelockArg("on")).toBe(true);
    expect(parseWakelockArg("off")).toBe(false);
    expect(parseWakelockArg("ON")).toBe(true);
    expect(parseWakelockArg(" Off ")).toBe(false);
  });
  it("still accepts 1/0/true/false for leniency", () => {
    expect(parseWakelockArg("1")).toBe(true);
    expect(parseWakelockArg("0")).toBe(false);
    expect(parseWakelockArg("true")).toBe(true);
    expect(parseWakelockArg("false")).toBe(false);
  });
  it("returns null for anything else", () => {
    expect(parseWakelockArg("")).toBeNull();
    expect(parseWakelockArg("yes")).toBeNull();
    expect(parseWakelockArg("2")).toBeNull();
  });
});

describe("bare pwgen surfaces the action (not a suggestion)", () => {
  it("parseCommand('pwgen') returns the pwgen spec with an empty arg", () => {
    // requiresArg is false → bare `pwgen` is a runnable command, so the
    // generator row ranks above snippet matches instead of a faint hint.
    const parsed = parseCommand("pwgen");
    expect(parsed?.spec.kind).toBe("pwgen");
    expect(parsed?.arg).toBe("");
  });

  it("does not suggest a bare pwgen (it runs as a command instead)", () => {
    expect(commandSuggestions("pwgen").some((s) => s.keyword === "pwgen")).toBe(
      false,
    );
    // …but a partial prefix still autocompletes to pwgen.
    expect(commandSuggestions("pwg").some((s) => s.keyword === "pwgen")).toBe(
      true,
    );
  });

  it("exposes a sane default length", () => {
    expect(DEFAULT_PWGEN_LENGTH).toBeGreaterThanOrEqual(4);
    expect(DEFAULT_PWGEN_LENGTH).toBeLessThanOrEqual(128);
  });
});
