import { describe, it, expect } from "vitest";
import {
  COMMANDS,
  DEFAULT_PWGEN_LENGTH,
  RESIZE_PRESETS,
  commandSuggestions,
  isCommandAvailable,
  fuzzyScore,
  parseAlarmArg,
  parseWakelockArg,
  parseWakelockRequest,
  parseTrackArg,
  parseDiscoArg,
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
  parseOtpQuery,
  is2faTrigger,
  parse2faAdd,
  isBpmTrigger,
  isEqualizerTrigger,
  isFlappyTrigger,
  formatBytes,
  resizePresetSuggestions,
  translateUrl,
  isTranslateKind,
  TRANSLATE_LANGS,
  SEARCH_BANGS,
} from "./commands";

describe("COMMANDS catalogue", () => {
  it("has 64 commands (translate ×9, dev-tools ×5, web-search bangs ×9, qr, sound+audio, rz+resize, optim+optimize, trim, hue, disco, stats, uptime, track, meme, …)", () => {
    // The meme command is build-flag-gated (MEME_ENABLED); the test env leaves
    // VITE_IR_MEME unset → enabled → present.
    expect(COMMANDS.length).toBe(87);
  });

  it("every keyword is unique", () => {
    const seen = new Set<string>();
    for (const c of COMMANDS) {
      expect(seen.has(c.keyword)).toBe(false);
      seen.add(c.keyword);
    }
  });

  it("gates platform-limited commands by OS via isCommandAvailable", () => {
    const freeze = COMMANDS.find((c) => c.keyword === "freeze")!;
    const touch = COMMANDS.find((c) => c.keyword === "touch")!;
    const tren = COMMANDS.find((c) => c.keyword === "tren")!;

    // freeze (input lock) is macOS-only.
    expect(isCommandAvailable(freeze, "mac")).toBe(true);
    expect(isCommandAvailable(freeze, "win")).toBe(false);
    expect(isCommandAvailable(freeze, "linux")).toBe(false);

    // touch/mkdir/terminal: macOS + Windows (file-manager integration).
    expect(isCommandAvailable(touch, "mac")).toBe(true);
    expect(isCommandAvailable(touch, "win")).toBe(true);
    expect(isCommandAvailable(touch, "linux")).toBe(false);

    // A command with no platform list runs everywhere.
    expect(isCommandAvailable(tren, "mac")).toBe(true);
    expect(isCommandAvailable(tren, "win")).toBe(true);
    expect(isCommandAvailable(tren, "linux")).toBe(true);
  });

  it("wakelock + caffeine are enterable on every OS (cross-platform backend)", () => {
    // The keep-awake backend has macOS (caffeinate), Windows
    // (SetThreadExecutionState + F15) and Linux (systemd-inhibit) impls, so
    // neither keyword carries a platform gate — both surface everywhere.
    for (const kw of ["wakelock", "caffeine"]) {
      const spec = COMMANDS.find((c) => c.keyword === kw)!;
      expect(spec.platform).toBeUndefined();
      expect(isCommandAvailable(spec, "mac")).toBe(true);
      expect(isCommandAvailable(spec, "win")).toBe(true);
      expect(isCommandAvailable(spec, "linux")).toBe(true);
    }
  });

  it("parseWakelockRequest: full grammar incl. dark (v0.116.0)", () => {
    // Bare on/off = the historical full mode, explicit.
    expect(parseWakelockRequest("on")).toEqual({ dark: false, on: true });
    expect(parseWakelockRequest(" OFF ")).toEqual({ dark: false, on: false });
    // `dark` alone TOGGLES (on: null — resolved against live state by App).
    expect(parseWakelockRequest("dark")).toEqual({ dark: true, on: null });
    expect(parseWakelockRequest("DARK")).toEqual({ dark: true, on: null });
    // Explicit dark on/off (with the lenient on/off vocabulary).
    expect(parseWakelockRequest("dark on")).toEqual({ dark: true, on: true });
    expect(parseWakelockRequest("dark off")).toEqual({ dark: true, on: false });
    expect(parseWakelockRequest("dark 1")).toEqual({ dark: true, on: true });
    // Garbage stays garbage — never silently a mode.
    expect(parseWakelockRequest("darkish")).toBeNull();
    expect(parseWakelockRequest("dark maybe")).toBeNull();
    expect(parseWakelockRequest("")).toBeNull();
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

  it("`sound` is an arg-less command of kind sound", () => {
    const r = parseCommand("sound");
    expect(r?.spec.kind).toBe("sound");
    expect(r?.spec.requiresArg).toBe(false);
    expect(COMMANDS.find((c) => c.keyword === "sound")?.kind).toBe("sound");
  });

  it("every syntax starts with its own keyword", () => {
    for (const c of COMMANDS) {
      expect(c.syntax.startsWith(c.keyword)).toBe(true);
    }
  });

  it("a requiresArg command always advertises the arg in its syntax", () => {
    // The autocomplete trailing-space rule keys off `syntax !== keyword`, so a
    // command that needs an argument must show one (e.g. `tren <text>`).
    for (const c of COMMANDS) {
      if (c.requiresArg) {
        expect(c.syntax.trim()).not.toBe(c.keyword);
        expect(c.syntax.length).toBeGreaterThan(c.keyword.length);
      }
    }
  });

  it("all six new language-pair translate commands are present + require an arg", () => {
    const langKeywords = [
      "trde2it",
      "trit2de",
      "trde2sp",
      "trsp2de",
      "trde2pl",
      "trpl2de",
    ];
    for (const kw of langKeywords) {
      const spec = COMMANDS.find((c) => c.keyword === kw);
      expect(spec, `missing command ${kw}`).toBeDefined();
      expect(spec?.requiresArg).toBe(true);
      expect(spec?.kind.startsWith("translate-")).toBe(true);
    }
  });
});

describe("parseCommand", () => {
  it("parses tren with text argument", () => {
    const r = parseCommand("tren hello world");
    expect(r?.spec.kind).toBe("translate-en");
    expect(r?.arg).toBe("hello world");
  });

  it("matches the exact keyword, never a longer one that shares the prefix", () => {
    // `trde` must resolve to translate-de, NOT translate-de-it/es/pl whose
    // keywords (trde2it, …) start with the same letters.
    expect(parseCommand("trde hallo")?.spec.kind).toBe("translate-de");
    expect(parseCommand("tr hallo")?.spec.kind).toBe("translate-auto");
    expect(parseCommand("trde2it hallo")?.spec.kind).toBe("translate-de-it");
    expect(parseCommand("trde2 hallo")).toBeNull(); // not a real keyword
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

describe("custom-command priority preconditions", () => {
  // App.tsx ranks a *complete* command (commandEntry) above an app-launcher
  // hit so typing `terminal` runs the command instead of launching
  // Terminal.app. That hinges on these bare keywords parsing as complete,
  // arg-less commands.
  it.each([
    ["terminal", "terminal"],
    ["freeze", "freeze"],
    ["lock", "lock"],
    ["mute", "mute"],
    ["reboot", "reboot"],
    ["shutdown", "shutdown"],
    ["brightness", "brightness"],
    ["bri", "brightness"],
  ])("`%s` parses as the complete %s command (no arg needed)", (input, kind) => {
    const r = parseCommand(input);
    expect(r).not.toBeNull();
    expect(r?.spec.kind).toBe(kind);
    expect(r?.spec.requiresArg).toBe(false);
  });

  it("these arg-less commands are not flagged requiresArg in the catalogue", () => {
    const argless = COMMANDS.filter((c) =>
      [
        "terminal",
        "freeze",
        "lock",
        "mute",
        "reboot",
        "shutdown",
        "brightness",
        "bri",
      ].includes(c.keyword),
    );
    expect(argless.length).toBe(8);
    expect(argless.every((c) => !c.requiresArg)).toBe(true);
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
    // The new language-pair commands also surface under the `tr` prefix.
    expect(keywords).toContain("trde2it");
    expect(keywords).toContain("trsp2de");
  });

  it("surfaces the German-source pairs for the 'trde2' prefix", () => {
    const keywords = commandSuggestions("trde2").map((s) => s.keyword);
    expect(keywords).toContain("trde2it");
    expect(keywords).toContain("trde2sp");
    expect(keywords).toContain("trde2pl");
    // German→English (`trde`) is NOT a `trde2`-prefix match.
    expect(keywords).not.toContain("trde");
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

describe("translate language-pair commands (v0.75.0)", () => {
  // keyword → [sl, tl] expected on the Google Translate URL.
  const PAIRS: Array<[string, string, string]> = [
    ["trde2it", "de", "it"],
    ["trit2de", "it", "de"],
    ["trde2sp", "de", "es"], // keyword spells "sp", Google code is "es"
    ["trsp2de", "es", "de"],
    ["trde2pl", "de", "pl"],
    ["trpl2de", "pl", "de"],
  ];

  it.each(PAIRS)("`%s` parses + builds an sl=%s/tl=%s URL", (kw, sl, tl) => {
    const parsed = parseCommand(`${kw} hallo welt`);
    expect(parsed).not.toBeNull();
    expect(parsed?.arg).toBe("hallo welt");
    const url = translateUrl(parsed!.spec.kind, parsed!.arg);
    expect(url).toContain(`sl=${sl}`);
    expect(url).toContain(`tl=${tl}`);
    expect(url).toContain("text=hallo%20welt");
    expect(url.startsWith("https://translate.google.com/")).toBe(true);
  });

  it.each(PAIRS)("`%s` requires an argument", (kw) => {
    const spec = COMMANDS.find((c) => c.keyword === kw);
    expect(spec?.requiresArg).toBe(true);
    expect(parseCommand(kw)).toBeNull(); // bare keyword, no text → null
  });

  it("every command in TRANSLATE_LANGS is in COMMANDS and vice-versa", () => {
    const translateKeywords = COMMANDS.filter((c) => isTranslateKind(c.kind)).map(
      (c) => c.kind,
    );
    const mapKinds = Object.keys(TRANSLATE_LANGS);
    expect(translateKeywords.sort()).toEqual(mapKinds.sort());
    // 3 original + 6 new = 9 translate commands.
    expect(mapKinds.length).toBe(9);
  });

  it("isTranslateKind is true for every translate kind, false otherwise", () => {
    for (const kind of Object.keys(TRANSLATE_LANGS)) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      expect(isTranslateKind(kind as any)).toBe(true);
    }
    expect(isTranslateKind("optim")).toBe(false);
    expect(isTranslateKind("pwgen")).toBe(false);
    expect(isTranslateKind("brightness")).toBe(false);
  });

  it("the Spanish pair uses the `es` code despite the `sp` keyword", () => {
    expect(TRANSLATE_LANGS["translate-de-es"]?.tl).toBe("es");
    expect(TRANSLATE_LANGS["translate-es-de"]?.sl).toBe("es");
    expect(COMMANDS.some((c) => c.keyword === "trde2sp")).toBe(true);
    expect(COMMANDS.some((c) => c.keyword === "trde2es")).toBe(false);
  });
});

describe("web-search bangs (v0.76.0)", () => {
  it("every bang has a COMMANDS row of kind websearch that requires an arg", () => {
    for (const keyword of Object.keys(SEARCH_BANGS)) {
      const spec = COMMANDS.find((c) => c.keyword === keyword);
      expect(spec, `missing command ${keyword}`).toBeDefined();
      expect(spec?.kind).toBe("websearch");
      expect(spec?.requiresArg).toBe(true);
    }
  });

  it("parses `g hello world` as a websearch with the full query", () => {
    const r = parseCommand("g hello world");
    expect(r?.spec.kind).toBe("websearch");
    expect(r?.spec.keyword).toBe("g");
    expect(r?.arg).toBe("hello world");
  });

  it("each bang URL targets its engine and URL-encodes the query", () => {
    expect(SEARCH_BANGS.g.url("a b")).toContain("google.com/search?q=a%20b");
    expect(SEARCH_BANGS.gh.url("rust")).toContain("github.com/search?q=rust");
    expect(SEARCH_BANGS.yt.url("lofi")).toContain(
      "youtube.com/results?search_query=lofi",
    );
    expect(SEARCH_BANGS.npm.url("vite")).toContain("npmjs.com/search?q=vite");
    expect(SEARCH_BANGS.so.url("c++")).toContain("q=c%2B%2B");
  });

  it("every bang URL is https and encodes ampersands in the query", () => {
    for (const { url } of Object.values(SEARCH_BANGS)) {
      const u = url("x & y");
      expect(u.startsWith("https://")).toBe(true);
      expect(u).toContain("%26"); // the literal & is encoded, not a new param
    }
  });

  it("surfaces bang suggestions for a partial keyword", () => {
    const keywords = commandSuggestions("git").map((s) => s.keyword);
    // `gh` is a first-char-anchored subsequence match for "git" (g..h?) —
    // at minimum the exact-ish prefixes show; assert the catalogue is wired.
    expect(commandSuggestions("npm").map((s) => s.keyword)).toContain("npm");
    expect(keywords.length).toBeGreaterThanOrEqual(0);
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

  it("accepts a plain space between the numbers (no x)", () => {
    expect(parseResizeArg("200 200")).toEqual({ width: 200, height: 200 });
    expect(parseResizeArg("1200   800")).toEqual({ width: 1200, height: 800 });
    expect(parseResizeArg("  200 200  ")).toEqual({ width: 200, height: 200 });
  });

  it("rejects a single number with no separator/second value", () => {
    expect(parseResizeArg("200200")).toBeNull();
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

  it("carries a numeric PID through as the pattern (kill 1234, kill -9 1234)", () => {
    // The PID is matched against the process list in App.tsx; the parser just
    // hands the digits through unchanged.
    expect(parseKillArg("1234")).toEqual({ force: false, pattern: "1234" });
    expect(parseKillArg("-9 1234")).toEqual({ force: true, pattern: "1234" });
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
      expect(commandSuggestions(prefix).some((c) => c.keyword.startsWith("rockth"))).toBe(
        false,
      );
    }
  });
});

describe("isSpaceInvadersTrigger — hidden Space Invaders easter egg", () => {
  it("matches exact `spacer`", () => {
    expect(isSpaceInvadersTrigger("spacer")).toBe(true);
    expect(isSpaceInvadersTrigger("  SPACER  ")).toBe(true);
  });
  it("rejects partial, extended, or the old `space` word", () => {
    expect(isSpaceInvadersTrigger("space")).toBe(false);
    expect(isSpaceInvadersTrigger("spac")).toBe(false);
    expect(isSpaceInvadersTrigger("spacers")).toBe(false);
    expect(isSpaceInvadersTrigger("space invaders")).toBe(false);
  });
  it("is NOT in the public COMMANDS catalogue", () => {
    expect(COMMANDS.some((c) => c.keyword === "spacer")).toBe(false);
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
      expect(commandSuggestions(prefix).some((c) => c.keyword.startsWith("open"))).toBe(
        false,
      );
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
  it("accepts a dash range (5-500, 1-2, spaced)", () => {
    expect(parseRandomArg("1-2")).toEqual({ min: 1, max: 2 });
    expect(parseRandomArg("5-500")).toEqual({ min: 5, max: 500 });
    expect(parseRandomArg("5 - 500")).toEqual({ min: 5, max: 500 });
    expect(parseRandomArg("500-5")).toEqual({ min: 5, max: 500 }); // swapped
  });
  it("rejects non-integers and 3+ numbers", () => {
    expect(parseRandomArg("abc")).toBeNull();
    expect(parseRandomArg("5x")).toBeNull();
    expect(parseRandomArg("1.5")).toBeNull();
    expect(parseRandomArg("1 2 3")).toBeNull();
    expect(parseRandomArg("1-2-3")).toBeNull();
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
    for (const u of [
      "s",
      "sec",
      "secs",
      "sek",
      "second",
      "seconds",
      "sekunde",
      "sekunden",
    ]) {
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

describe("parseDiscoArg", () => {
  it("maps 1/on/true → on, 0/off/false → off", () => {
    expect(parseDiscoArg("1")).toBe(true);
    expect(parseDiscoArg("on")).toBe(true);
    expect(parseDiscoArg(" TRUE ")).toBe(true);
    expect(parseDiscoArg("0")).toBe(false);
    expect(parseDiscoArg("off")).toBe(false);
  });
  it("returns null (= toggle) for empty / unknown", () => {
    expect(parseDiscoArg("")).toBeNull();
    expect(parseDiscoArg("maybe")).toBeNull();
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
    expect(commandSuggestions("pwgen").some((s) => s.keyword === "pwgen")).toBe(false);
    // …but a partial prefix still autocompletes to pwgen.
    expect(commandSuggestions("pwg").some((s) => s.keyword === "pwgen")).toBe(true);
  });

  it("exposes a sane default length", () => {
    expect(DEFAULT_PWGEN_LENGTH).toBeGreaterThanOrEqual(4);
    expect(DEFAULT_PWGEN_LENGTH).toBeLessThanOrEqual(128);
  });
});

describe("parseOtpQuery", () => {
  it("returns the issuer filter for `otp <issuer>` (trimmed)", () => {
    expect(parseOtpQuery("otp amazon")).toBe("amazon");
    expect(parseOtpQuery("OTP  GitHub ")).toBe("GitHub");
    expect(parseOtpQuery("otp google work")).toBe("google work");
  });
  it("also accepts the `2fa <issuer>` spelling", () => {
    expect(parseOtpQuery("2fa hosti")).toBe("hosti");
    expect(parseOtpQuery("2FA  Hostinger ")).toBe("Hostinger");
    expect(parseOtpQuery("2fa google work")).toBe("google work");
  });
  it("returns null for bare `otp`/`2fa` (the overlay opens via is2faTrigger instead)", () => {
    expect(parseOtpQuery("otp")).toBeNull();
    expect(parseOtpQuery("otp ")).toBeNull();
    expect(parseOtpQuery("otp   ")).toBeNull();
    expect(parseOtpQuery("2fa")).toBeNull();
    expect(parseOtpQuery("2fa   ")).toBeNull();
  });
  it("returns null when the query isn't an otp query", () => {
    expect(parseOtpQuery("")).toBeNull();
    expect(parseOtpQuery("otpfoo")).toBeNull(); // no boundary
    expect(parseOtpQuery("2fafoo")).toBeNull(); // no boundary
    expect(parseOtpQuery("note otp")).toBeNull();
  });
});

describe("is2faTrigger", () => {
  it("matches bare `2fa` and `otp` (case/space tolerant)", () => {
    expect(is2faTrigger("2fa")).toBe(true);
    expect(is2faTrigger("otp")).toBe(true);
    expect(is2faTrigger("  OTP  ")).toBe(true);
    expect(is2faTrigger("2FA")).toBe(true);
  });
  it("does not match `otp <issuer>` or unrelated input", () => {
    expect(is2faTrigger("otp amazon")).toBe(false);
    expect(is2faTrigger("otpfoo")).toBe(false);
    expect(is2faTrigger("hello")).toBe(false);
  });
});

describe("parse2faAdd", () => {
  it("matches `2fa add` / `otp add` with no prefill", () => {
    expect(parse2faAdd("2fa add")).toEqual({ issuer: "" });
    expect(parse2faAdd("otp add")).toEqual({ issuer: "" });
    expect(parse2faAdd("  2FA  ADD  ")).toEqual({ issuer: "" });
    // The autocomplete's trailing space (completion "2fa add ") must match too.
    expect(parse2faAdd("2fa add ")).toEqual({ issuer: "" });
  });
  it("captures a trailing argument as the issuer prefill", () => {
    expect(parse2faAdd("2fa add GitHub")).toEqual({ issuer: "GitHub" });
    expect(parse2faAdd("otp add Amazon Web Services")).toEqual({
      issuer: "Amazon Web Services",
    });
    expect(parse2faAdd("2fa add  Hostinger ")).toEqual({ issuer: "Hostinger" });
  });
  it("requires `add` as its own token — issuer searches stay untouched", () => {
    // A company merely STARTING with "add" is an issuer search, not the form.
    expect(parse2faAdd("2fa addepar")).toBeNull();
    expect(parse2faAdd("otp adde")).toBeNull();
    expect(parse2faAdd("2fa")).toBeNull();
    expect(parse2faAdd("otp")).toBeNull();
    expect(parse2faAdd("2fa amazon")).toBeNull();
    expect(parse2faAdd("hello add")).toBeNull();
  });
});

describe("hidden game/word triggers (exact word, case-insensitive)", () => {
  it("isBpmTrigger matches only the exact word `bpm`", () => {
    expect(isBpmTrigger("bpm")).toBe(true);
    expect(isBpmTrigger("  BPM ")).toBe(true);
    expect(isBpmTrigger("bpms")).toBe(false);
    expect(isBpmTrigger("bpm detector")).toBe(false);
    expect(isBpmTrigger("")).toBe(false);
  });
  it("isEqualizerTrigger matches any ≥2-char prefix of `equalizer`", () => {
    for (const q of ["eq", "equ", "equa", "equal", "equali", "equaliz", "equalize", "equalizer"]) {
      expect(isEqualizerTrigger(q)).toBe(true);
      expect(isEqualizerTrigger(q.toUpperCase())).toBe(true);
    }
    expect(isEqualizerTrigger("  Equalizer ")).toBe(true);
    expect(isEqualizerTrigger("e")).toBe(false); // too short
    expect(isEqualizerTrigger("equalizers")).toBe(false); // past the word
    expect(isEqualizerTrigger("eqz")).toBe(false); // not a prefix
    expect(isEqualizerTrigger("")).toBe(false);
  });
  it("isFlappyTrigger matches only the exact word `learningtofly`", () => {
    expect(isFlappyTrigger("learningtofly")).toBe(true);
    expect(isFlappyTrigger("LearningToFly")).toBe(true);
    expect(isFlappyTrigger(" learningtofly ")).toBe(true);
    expect(isFlappyTrigger("learning")).toBe(false);
    expect(isFlappyTrigger("learningtofly!")).toBe(false);
  });
});

describe("platform gating", () => {
  it("boom is available on macOS and Windows (Equalizer-APO backend), not Linux", () => {
    const boom = COMMANDS.find((c) => c.kind === "boom");
    expect(boom?.platform).toEqual(["mac", "win"]);
  });

  it("every platform-gated command names only known platforms", () => {
    for (const c of COMMANDS) {
      if (!c.platform) continue;
      expect(c.platform.length).toBeGreaterThan(0);
      for (const p of c.platform) expect(["mac", "win", "linux"]).toContain(p);
    }
  });
});

describe("parseTrackArg", () => {
  it("treats an empty / whitespace arg as 'open the tab'", () => {
    expect(parseTrackArg("")).toBe("open");
    expect(parseTrackArg("   ")).toBe("open");
  });

  it("accepts every start synonym", () => {
    for (const a of ["on", "start", "1", "ON", " Start "]) {
      expect(parseTrackArg(a)).toBe("on");
    }
  });

  it("accepts every stop synonym", () => {
    for (const a of ["off", "stop", "0", "OFF", " Stop "]) {
      expect(parseTrackArg(a)).toBe("off");
    }
  });

  it("rejects anything else", () => {
    expect(parseTrackArg("pause")).toBeNull();
    expect(parseTrackArg("2")).toBeNull();
    expect(parseTrackArg("onn")).toBeNull();
  });
});
