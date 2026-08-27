import { describe, it, expect } from "vitest";
// Vite's `?raw` — the same mechanism the x!-renderer pin uses, so this runs
// in vitest AND typechecks (the frontend tsconfig has no Node types).
import readmeEn from "../../../../README.md?raw";
import readmeDe from "../../../../README.de.md?raw";
import { COMMAND_DOCS } from "./commandDocs";
import { COMMANDS } from "./commands";

/**
 * Badges are documentation, and documentation that quietly stops being true is
 * worse than none — a reader trusts a number more than prose.
 *
 * `scripts/update-badges.mjs` recomputes these from the real sources, but it
 * only runs as a `posttest` hook on a machine that runs the full suite. These
 * assertions make the drift fail in CI as well, and they read the SAME
 * registry the README matrix is generated from.
 */
const badge = (readme: string, key: string): number | null => {
  const m = readme.match(new RegExp(`badge/${key}-(\\d+)`));
  return m ? Number(m[1]) : null;
};

describe("README badges tell the truth", () => {
  for (const [lang, readme] of [
    ["README.md", readmeEn],
    ["README.de.md", readmeDe],
  ] as const) {
    it(`${lang}: the commands badge equals the documented commands`, () => {
      expect(badge(readme, "commands"), "commands badge missing").not.toBeNull();
      expect(badge(readme, "commands")).toBe(COMMAND_DOCS.length);
    });

    it(`${lang}: the docs badge is present and plausible`, () => {
      // The page count itself lives on disk; assert only that the badge is
      // there and not a placeholder — claiming a number we cannot see from
      // here would be exactly the dishonesty this file exists to prevent.
      const m = readme.match(/badge\/docs-(\d+)%20pages/);
      expect(m, "docs badge missing").not.toBeNull();
      expect(Number(m![1])).toBeGreaterThan(0);
    });

    it(`${lang}: the test-count badge adds up`, () => {
      const m = readme.match(/unit%20tests-(\d+)%20\((\d+)%20Rust%20%2B%20(\d+)%20TS\)/);
      expect(m, "unit-test badge missing").not.toBeNull();
      const [total, rust, ts] = m!.slice(1).map(Number);
      expect(rust + ts, "total must equal the two runners").toBe(total);
    });
  }

  it("every documented command really exists in the catalogue", () => {
    // The badge counts docs; this makes sure that number describes something
    // real rather than a registry that has drifted from the commands.
    const keywords = new Set<string>();
    for (const c of COMMANDS) keywords.add(c.keyword);
    for (const d of COMMAND_DOCS) {
      expect(keywords.has(d.command), `${d.command} hat keinen Befehl`).toBe(true);
      for (const a of d.aliases) {
        expect(keywords.has(a), `Alias ${a} von ${d.command} fehlt im Katalog`).toBe(true);
      }
    }
  });
});

/**
 * The docs index is the only way a reader finds the 24 reference pages — an
 * index that silently misses a page is a page that does not exist. `?raw`
 * globbing reads the REAL directory, so adding a file without linking it
 * fails here rather than being noticed months later.
 */
const docFiles = Object.keys(
  import.meta.glob("../../../../docs/*.md", { query: "?raw", eager: false }),
).map((p) => p.split("/").pop()!);

describe("the docs index is complete", () => {
  it("finds the docs directory at all (guards the glob itself)", () => {
    // A glob that silently matches nothing would make every assertion below
    // vacuously true — the classic green-blind trap.
    expect(docFiles.length).toBeGreaterThan(15);
    expect(docFiles).toContain("reports.md");
  });

  for (const [lang, readme] of [
    ["README.md", readmeEn],
    ["README.de.md", readmeDe],
  ] as const) {
    it(`${lang}: every page under docs/ is linked`, () => {
      const missing = docFiles.filter((f) => !readme.includes(`./docs/${f}`));
      expect(missing, `nicht verlinkt: ${missing.join(", ")}`).toEqual([]);
    });

    it(`${lang}: the index links no page that does not exist`, () => {
      const linked = [...readme.matchAll(/\.\/docs\/([\w.-]+\.md)/g)].map((m) => m[1]);
      const ghosts = [...new Set(linked)].filter((f) => !docFiles.includes(f));
      expect(ghosts, `toter Verweis: ${ghosts.join(", ")}`).toEqual([]);
    });
  }
});
