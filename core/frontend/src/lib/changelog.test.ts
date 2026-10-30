import { describe, expect, it } from "vitest";
import { filterReleases, parseChangelog, stripLinks } from "./changelog";
// The same file the Settings section loads (repo root).
import FILE from "../../../../CHANGELOG.md?raw";
import PKG_RAW from "../../../../package.json?raw";

const PKG = JSON.parse(PKG_RAW) as { version: string };

describe("parseChangelog", () => {
  it("reads release headings with every dash and drops an empty Unreleased", () => {
    const r = parseChangelog(
      "# Changelog\nintro\n\n## [Unreleased]\n\n## [1.2.0] — 2026-01-02\n- a\n\n## [1.1.0] – 2026-01-01\n- b\n\n## [1.0.0] - 2025-12-31\n- c\n",
    );
    expect(r.map((x) => [x.version, x.date])).toEqual([
      ["1.2.0", "2026-01-02"],
      ["1.1.0", "2026-01-01"],
      ["1.0.0", "2025-12-31"],
    ]);
  });

  it("keeps sub-headings, nesting, wrapped lines and numbered items", () => {
    const [r] = parseChangelog(
      "## [1.0.0] - 2026-01-01\n### Added\n- **One** line\n  wrapped on\n  - nested\n1. first\n2. second\n#### Detail\nA paragraph\nthat wraps.\n",
    );
    expect(r.blocks).toEqual([
      { kind: "heading", text: "Added" },
      { kind: "item", text: "**One** line wrapped on", depth: 0, ordered: false },
      { kind: "item", text: "nested", depth: 1, ordered: false },
      { kind: "item", text: "first", depth: 0, ordered: true },
      { kind: "item", text: "second", depth: 0, ordered: true },
      { kind: "heading", text: "Detail" },
      { kind: "para", text: "A paragraph that wraps." },
    ]);
  });

  it("keeps code verbatim and turns tables into rows without the rule line", () => {
    const [r] = parseChangelog(
      "## [1.0.0] - 2026-01-01\n```\n  a  b\n```\n| x | y |\n|---|:-:|\n| 1 | [l](https://e.x) |\n",
    );
    expect(r.blocks).toEqual([
      { kind: "code", text: "  a  b" },
      { kind: "table", rows: [["x", "y"], ["1", "l"]] },
    ]);
  });

  it("reduces links to their label", () => {
    expect(stripLinks("see [the docs](https://x.y/z) and [b](c)")).toBe("see the docs and b");
  });

  it("filters by version, date and text, case-insensitively", () => {
    const rs = parseChangelog("## [2.0.0] - 2026-02-01\n- Faster OCR\n\n## [1.0.0] - 2026-01-01\n- First\n");
    expect(filterReleases(rs, "ocr").map((r) => r.version)).toEqual(["2.0.0"]);
    expect(filterReleases(rs, "2026-01").map((r) => r.version)).toEqual(["1.0.0"]);
    expect(filterReleases(rs, "  ")).toHaveLength(2);
  });
});

describe("CHANGELOG.md as shown in Settings", () => {
  const releases = parseChangelog(FILE);

  it("parses every release and leads with the current version", () => {
    expect(releases.length).toBeGreaterThan(500);
    expect(releases[0].version).toBe(PKG.version);
    expect(releases.every((r) => r.blocks.length > 0)).toBe(true);
  });

  // Prose only: text inside backticks is code, file names and literal input.
  const prose = FILE.split("\n").map((l) => l.replace(/`[^`]*`/g, " "));
  const offending = (re: RegExp) =>
    prose.map((l, i) => [i + 1, l] as const).filter(([, l]) => re.test(l)).map(([n, l]) => `${n}: ${l.slice(0, 120)}`);

  it("contains no secrets", () => {
    const secret =
      /(sk-[A-Za-z0-9]{16,}|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|github_pat_\w{20,}|xox[abp]-[\w-]{10,}|AKIA[0-9A-Z]{16}|AIza[\w-]{30,}|eyJ[\w-]{20,}\.[\w-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY|(api[_-]?key|token|secret|password|passwd)\s*[:=]\s*["']?[A-Za-z0-9_\-+/]{12,})/i;
    expect(offending(secret)).toEqual([]);
    // Code spans are scanned too: a secret in backticks is still a secret.
    expect(FILE.split("\n").filter((l) => /(sk-[A-Za-z0-9]{16,}|ghp_[A-Za-z0-9]{20,}|BEGIN [A-Z ]*PRIVATE KEY)/.test(l))).toEqual([]);
  });

  it("names no private infrastructure", () => {
    // IPv4 only from the documentation ranges (RFC 5737) or loopback.
    const ips = FILE.match(/\b\d{1,3}(?:\.\d{1,3}){3}\b/g) ?? [];
    const notDoc = ips.filter((ip) => !/^(192\.0\.2|198\.51\.100|203\.0\.113)\.|^127\.0\.0\.1$/.test(ip));
    // Version-like tokens (e.g. 0.84.1.2) never occur; anything left is an address.
    expect(notDoc).toEqual([]);
    expect(FILE.match(/\/Users\/[A-Za-z]{3,}|\/home\/[a-z]{3,}\//g) ?? []).toEqual([]);
    const mails = (FILE.match(/[\w.%+-]+@[\w-]+\.[\w.-]+/g) ?? []).filter((m) => m !== "git@github.com");
    expect(mails).toEqual([]);
  });

  it("is written in English", () => {
    expect(offending(/[„“]/)).toEqual([]);
    // Umlauts only in people's names (cited authors).
    const NAMES = /Böck/g;
    expect(offending(/[äöüÄÖÜß]/).filter((l) => /[äöüÄÖÜß]/.test(l.replace(NAMES, "")))).toEqual([]);
    const de = /\b(der|die|das|und|ist|nicht|wird|werden|mit|auf|für|ein|eine|jetzt|bei|auch|nur|wenn|sich|dem|den|von|zu|im|sind|kein|keine|statt|bleibt|wurde)\b/g;
    const en = /\b(the|and|is|not|with|for|now|when|only|this|that|from|are|was|of|to|in)\b/g;
    const german = prose
      .map((l, i) => ({ n: i + 1, l, d: (l.match(de) ?? []).length, e: (l.match(en) ?? []).length }))
      .filter((x) => x.d >= 2 && x.d > x.e)
      .map((x) => `${x.n}: ${x.l.slice(0, 120)}`);
    expect(german).toEqual([]);
  });
});
