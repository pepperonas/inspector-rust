import { describe, it, expect } from "vitest";
import {
  COMMANDS,
  commandSuggestions,
  parseCommand,
  parseResizeArg,
  translateUrl,
} from "./commands";

describe("COMMANDS catalogue", () => {
  it("has 6 commands", () => {
    expect(COMMANDS.length).toBe(6);
  });

  it("every keyword is unique", () => {
    const seen = new Set<string>();
    for (const c of COMMANDS) {
      expect(seen.has(c.keyword)).toBe(false);
      seen.add(c.keyword);
    }
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
