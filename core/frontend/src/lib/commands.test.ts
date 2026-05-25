import { describe, it, expect } from "vitest";
import {
  COMMANDS,
  RESIZE_PRESETS,
  commandSuggestions,
  isGetShakyTrigger,
  isOpenerTrigger,
  isSpaceInvadersTrigger,
  rockTheBoxMode,
  parseCommand,
  parseKillArg,
  parseResizeArg,
  parsePwgenArg,
  parseTimerArg,
  resizePresetSuggestions,
  translateUrl,
} from "./commands";

describe("COMMANDS catalogue", () => {
  it("has 19 commands (12 base + 4 wakelock entries + bruno + timer + pwgen)", () => {
    expect(COMMANDS.length).toBe(19);
  });

  it("every keyword is unique", () => {
    const seen = new Set<string>();
    for (const c of COMMANDS) {
      expect(seen.has(c.keyword)).toBe(false);
      seen.add(c.keyword);
    }
  });

  it("wakelock has both canonical (=) and hidden (no-=) spellings", () => {
    const on = COMMANDS.filter((c) => c.kind === "wakelock-on");
    const off = COMMANDS.filter((c) => c.kind === "wakelock-off");
    expect(on.map((c) => c.keyword).sort()).toEqual(["wakelock1", "wakelock=1"]);
    expect(off.map((c) => c.keyword).sort()).toEqual(["wakelock0", "wakelock=0"]);
    // Aliases are hidden from autocomplete.
    expect(on.find((c) => c.keyword === "wakelock1")?.hidden).toBe(true);
    expect(on.find((c) => c.keyword === "wakelock=1")?.hidden).toBeFalsy();
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

  it("matches only tren for 'tre'", () => {
    const suggestions = commandSuggestions("tre");
    const keywords = suggestions.map((s) => s.keyword);
    expect(keywords).toEqual(["tren"]);
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
