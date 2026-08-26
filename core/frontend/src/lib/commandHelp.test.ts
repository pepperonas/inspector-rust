import { describe, it, expect } from "vitest";
import { parseHelpQuery, isHelpQuery } from "./commandHelp";

function doc(query: string): string | null {
  const t = parseHelpQuery(query);
  if (!t) return null;
  return t.kind === "index" ? "@index" : t.doc.command;
}

describe("parseHelpQuery — index", () => {
  it("`?` alone opens the index", () => {
    expect(doc("?")).toBe("@index");
    expect(doc("  ?  ")).toBe("@index");
  });
  it("`??` is not the index", () => {
    expect(doc("??")).toBeNull();
  });
});

describe("parseHelpQuery — exact command + `?`", () => {
  it("resolves a command directly (no space and one space)", () => {
    expect(doc("kill?")).toBe("kill");
    expect(doc("kill ?")).toBe("kill");
    expect(doc("faker ?")).toBe("faker");
    expect(doc("qr?")).toBe("qr");
  });
  it("is case-insensitive", () => {
    expect(doc("KILL?")).toBe("kill");
    expect(doc("Faker ?")).toBe("faker");
  });
  it("resolves hidden aliases too", () => {
    expect(doc("cal?")).toBe("calendar");
    expect(doc("nmap?")).toBe("sec");
    expect(doc("caffeine?")).toBe("wakelock");
    expect(doc("shotfull?")).toBe("shot");
  });
});

describe("parseHelpQuery — partial prefix + `?`", () => {
  it("resolves to the top autocomplete match's doc", () => {
    expect(doc("sni?")).toBe("snitch");
    expect(doc("bright?")).toBe("brightness");
    expect(doc("wake?")).toBe("wakelock");
  });
  it("returns null for a token with no command match", () => {
    expect(doc("zzzzz?")).toBeNull();
    expect(doc("qwxyz ?")).toBeNull();
  });
});

describe("parseHelpQuery — leading `?<command>` (no space)", () => {
  it("resolves the command doc directly, like `snitch?`", () => {
    expect(doc("?snitch")).toBe("snitch");
    expect(doc("?weather")).toBe("weather");
    expect(doc("?kill")).toBe("kill");
  });
  it("resolves hidden aliases exactly", () => {
    expect(doc("?cal")).toBe("calendar"); // hidden alias
    expect(doc("?repo")).toBe("repo");
  });

  it("a PREFIX filters the index — `?re` behaves like `? re` (v0.133.0)", () => {
    // Field report: `? re` listed the matches but `?re` jumped into one doc.
    // The space after the `?` must not change the outcome.
    expect(parseHelpQuery("?re")).toEqual(parseHelpQuery("? re"));
    expect(parseHelpQuery("?re")).toEqual({ kind: "index", filter: "re" });
    expect(parseHelpQuery("?sni")).toEqual({ kind: "index", filter: "sni" });
    // …and that filter really does surface the commands you'd expect.
    const hits = searchDocs("re").map((d) => d.command);
    expect(hits).toContain("repo");
    expect(hits).toContain("reboot");
  });

  it("the TRAILING form keeps prefix resolution (a different intent)", () => {
    // `sni?` = "help for THIS command"; `?sni` = "search the docs".
    expect(doc("sni?")).toBe("snitch");
    expect(doc("bright?")).toBe("brightness");
  });
  it("is case-insensitive + whitespace-tolerant", () => {
    expect(doc("?SNITCH")).toBe("snitch");
    expect(doc("  ?weather  ")).toBe("weather");
  });
  it("an unmatched term still opens the (filtered) index — the leading ? is the tell", () => {
    expect(parseHelpQuery("?zzzzz")).toEqual({ kind: "index", filter: "zzzzz" });
  });
  it("`? term` WITH a space stays the full-text index search (unchanged)", () => {
    expect(parseHelpQuery("? snitch")).toEqual({ kind: "index", filter: "snitch" });
  });
});

describe("parseHelpQuery — `?` as a literal (no trigger)", () => {
  it("does not trigger inside a quoted template argument", () => {
    expect(doc('faker tpl "warum? {name}"')).toBeNull();
    expect(doc('faker tpl "{name}?"')).toBeNull();
  });
  it("does not trigger in a glob/regex", () => {
    expect(doc("a?b")).toBeNull();
    expect(doc("file?.txt")).toBeNull();
  });
  it("does not trigger inside or at the end of a URL", () => {
    expect(doc("https://x.com/?id=1")).toBeNull();
    expect(doc("https://x.com/?")).toBeNull(); // token has :/. → not command-shaped
  });
  it("does not trigger after an argument", () => {
    expect(doc("bruno hallo?")).toBeNull();
    expect(doc("tr guten morgen?")).toBeNull();
    expect(doc("faker tpl?")).toBeNull(); // arg present → literal
    expect(doc("faker tpl ?")).toBeNull(); // two tokens before ? → literal
  });
  it("does not trigger for multi-word queries", () => {
    expect(doc("hello world?")).toBeNull();
    expect(doc("what is this?")).toBeNull();
  });
  it("returns null for a plain query with no `?`", () => {
    expect(doc("kill")).toBeNull();
    expect(doc("")).toBeNull();
    expect(doc("faker person 50")).toBeNull();
  });
});

describe("isHelpQuery", () => {
  it("mirrors parseHelpQuery's boolean", () => {
    expect(isHelpQuery("?")).toBe(true);
    expect(isHelpQuery("kill?")).toBe(true);
    expect(isHelpQuery("sni?")).toBe(true);
    expect(isHelpQuery("bruno hallo?")).toBe(false);
    expect(isHelpQuery("a?b")).toBe(false);
    expect(isHelpQuery("kill")).toBe(false);
  });
});

// ── `? <term>` — filtered index + full-text search (v0.87.2) ────────────────

import { searchDocs, allDocs } from "./commandHelp";

describe("parseHelpQuery — `? <term>` filtered index", () => {
  it("`? <term>` yields the index with the filter", () => {
    expect(parseHelpQuery("? clip")).toEqual({ kind: "index", filter: "clip" });
    expect(parseHelpQuery("?  netz  ")).toEqual({ kind: "index", filter: "netz" });
  });
  it("bare `?` keeps an empty filter", () => {
    expect(parseHelpQuery("?")).toEqual({ kind: "index", filter: "" });
    expect(parseHelpQuery("  ?  ")).toEqual({ kind: "index", filter: "" });
  });
  it("a TRAILING `?` after an argument still stays literal", () => {
    expect(parseHelpQuery("bruno hallo?")).toBeNull();
    expect(parseHelpQuery("faker tpl \"warum? {name}\"")).toBeNull();
  });
  it("`?` mid-string never triggers the search form", () => {
    expect(parseHelpQuery("a ? b")).toBeNull(); // `a` is a token, but ? isn't trailing-lone
    expect(parseHelpQuery("https://x.de?id=1")).toBeNull();
  });
});

describe("searchDocs", () => {
  it("empty term returns every doc sorted alphabetically by command", () => {
    const alpha = [...allDocs()].sort((a, b) => a.command.localeCompare(b.command));
    expect(searchDocs("")).toEqual(alpha);
    expect(searchDocs("   ")).toEqual(alpha);
    // Same set as the registry, just reordered.
    expect(new Set(searchDocs("").map((d) => d.command))).toEqual(
      new Set(allDocs().map((d) => d.command)),
    );
    const cmds = searchDocs("").map((d) => d.command);
    expect(cmds).toEqual([...cmds].sort((a, b) => a.localeCompare(b)));
  });
  it("keyword matches rank first (exact > prefix)", () => {
    const kill = searchDocs("kill");
    expect(kill[0].command).toBe("kill");
    const set = searchDocs("sett");
    expect(set[0].command).toBe("settings");
  });
  it("aliases resolve to their primary doc", () => {
    expect(searchDocs("config")[0].command).toBe("settings");
    expect(searchDocs("caffeine")[0].command).toBe("wakelock");
  });

  it("matches ACROSS an inline-Markdown marker (v0.131.0)", () => {
    // adb's description reads `**Info** — live dashboard …`; searching the raw
    // string made a query spanning the `**` miss it entirely.
    const adb = allDocs().find((d) => d.command === "adb");
    expect(adb?.description).toContain("**Info**");
    expect(searchDocs("info — live").map((d) => d.command)).toContain("adb");
  });
  it("full-text: tagline/description matches surface, ranked below name hits", () => {
    const hits = searchDocs("clipboard");
    expect(hits.length).toBeGreaterThan(3);
    // Every hit really mentions the term somewhere in its doc.
    for (const d of hits) {
      const hay = [d.command, ...d.aliases, d.tagline, d.description, d.synopsis, ...d.tips]
        .join(" ")
        .toLowerCase();
      expect(hay).toContain("clip");
    }
  });
  it("garbage yields an empty list (the UI shows the fallback index)", () => {
    expect(searchDocs("zzzzqqqq")).toEqual([]);
  });
  it("is case-insensitive", () => {
    expect(searchDocs("KILL")[0].command).toBe("kill");
  });
});
