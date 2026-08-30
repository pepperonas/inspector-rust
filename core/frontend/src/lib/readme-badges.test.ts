import { describe, it, expect } from "vitest";
// Vite's `?raw` — the same mechanism the x!-renderer pin uses, so this runs
// in vitest AND typechecks (the frontend tsconfig has no Node types).
import readmeEn from "../../../../README.md?raw";
import readmeDe from "../../../../README.de.md?raw";
import featuresTxt from "../../../../features.txt?raw";
import changelogMd from "../../../../CHANGELOG.md?raw";
import cargoToml from "../../../../Cargo.toml?raw";
import rootPkg from "../../../../package.json?raw";
import fePkg from "../../package.json?raw";
import macPkg from "../../../../macos/package.json?raw";
import winPkg from "../../../../win/package.json?raw";
import linuxPkg from "../../../../linux/package.json?raw";
import macConf from "../../../../macos/src-tauri/tauri.conf.json?raw";
import winConf from "../../../../win/src-tauri/tauri.conf.json?raw";
import linuxConf from "../../../../linux/src-tauri/tauri.conf.json?raw";
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

describe("badge parity and manifest agreement", () => {
  it("features badge equals the features.txt line count, in BOTH languages", () => {
    // features.txt is the always-current one-line catalogue by contract; a
    // badge that disagrees with it is a lie about the catalogue's size.
    const lines = featuresTxt.split("\n").filter((l) => l.trim().length > 0).length;
    expect(lines, "features.txt must not be empty").toBeGreaterThan(0);
    expect(badge(readmeEn, "features")).toBe(lines);
    expect(badge(readmeDe, "features")).toBe(lines);
  });

  it("every computed badge shows the SAME value in English and German", () => {
    // ⚠️ This is the drift that actually happened: the German README sat at
    // docs-22/modules-84 beside the English 24/87 for weeks, because the
    // update script only carried rules for one language.
    for (const key of ["commands", "docs", "rust%20modules", "crates", "features"]) {
      expect(badge(readmeEn, key), `${key} missing in EN`).not.toBeNull();
      expect(badge(readmeDe, key), `${key} missing in DE`).toBe(badge(readmeEn, key));
    }
    const split = (r: string) => r.match(/badge\/Rust-~(\d+)k%20LoC/)?.[1];
    expect(split(readmeEn), "Rust split badge missing").toBeDefined();
    expect(split(readmeDe)).toBe(split(readmeEn));
  });

  it("all nine version manifests agree on ONE version", () => {
    // ⚠️ The release ritual bumps nine files; a missed one ships an app whose
    // bundle and package disagree about what it is.
    const pkgV = (s: string) => JSON.parse(s).version as string;
    const v = pkgV(rootPkg);
    expect(v).toMatch(/^\d+\.\d+\.\d+$/);
    for (const [name, s] of [
      ["core/frontend", fePkg], ["macos", macPkg], ["win", winPkg], ["linux", linuxPkg],
      ["macos tauri.conf", macConf], ["win tauri.conf", winConf], ["linux tauri.conf", linuxConf],
    ] as const) {
      expect(pkgV(s), name).toBe(v);
    }
    expect(cargoToml, "Cargo.toml workspace version").toContain(`version = "${v}"`);
  });

  it("the CHANGELOG's topmost entry is the current version", () => {
    // A release whose CHANGELOG still leads with the previous version has
    // documentation lagging the artefact.
    const v = JSON.parse(rootPkg).version as string;
    const top = changelogMd.match(/^## \[(\d+\.\d+\.\d+)\]/m)?.[1];
    expect(top, "no version heading in CHANGELOG").toBeDefined();
    expect(top).toBe(v);
  });
});
