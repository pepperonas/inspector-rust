// The product page (inspector-rust.celox.io) shows the lines of code and the unit-test count. Its
// release timer fetches `.github/repo-stats.json` from GitHub every 15 minutes (template
// `repo_stats`), so the numbers follow every push without a deploy — PROVIDED the file is current.
// `scripts/update-badges.mjs` writes it from the very numbers it writes into the README badges;
// these tests pin that the two never disagree, and that the file has the shape the timer accepts
// (non-negative integers, nothing else).
import { describe, expect, it } from "vitest";
import statsRaw from "../../../../.github/repo-stats.json?raw";
import readmeEn from "../../../../README.md?raw";
import siteJson from "../../../../website/site.json?raw";

const stats = JSON.parse(statsRaw) as Record<string, unknown>;
const num = (re: RegExp) => {
  const m = readmeEn.match(re);
  return m ? m.slice(1).map(Number) : null;
};

describe("repo-stats.json (website numbers)", () => {
  it("carries every number the page shows, as non-negative integers", () => {
    for (const k of ["loc", "loc_rust", "loc_ts", "tests", "tests_rust", "tests_frontend"]) {
      const v = stats[k];
      expect(Number.isInteger(v) && (v as number) >= 0, `${k} = ${String(v)}`).toBe(true);
    }
  });

  it("the test counts equal the README's", () => {
    const [total, rust, fe] = num(/unit%20tests-(\d+)%20\((\d+)%20Rust%20%2B%20(\d+)%20TS\)/) ?? [];
    expect(stats.tests).toBe(total);
    expect(stats.tests_rust).toBe(rust);
    expect(stats.tests_frontend).toBe(fe);
    expect(stats.tests).toBe((stats.tests_rust as number) + (stats.tests_frontend as number));
  });

  it("the line counts equal the README's", () => {
    const [locK] = num(/lines%20of%20code-~(\d+)k/) ?? [];
    expect(Math.round((stats.loc as number) / 1000)).toBe(locK);
    expect(stats.loc).toBe((stats.loc_rust as number) + (stats.loc_ts as number));
  });

  it("the catalogue counts equal the README's", () => {
    const [commands] = num(/badge\/commands-(\d+)/) ?? [];
    const [features] = num(/badge\/features-(\d+)/) ?? [];
    expect(stats.commands).toBe(commands);
    expect(stats.features).toBe(features);
  });

  it("the site reads this file and shows lines of code and tests", () => {
    const site = JSON.parse(siteJson) as { repo_stats?: { path: string; items: { key: string }[] } };
    expect(site.repo_stats?.path).toBe(".github/repo-stats.json");
    const keys = site.repo_stats?.items.map((i) => i.key) ?? [];
    expect(keys).toEqual(expect.arrayContaining(["loc", "tests"]));
    for (const k of keys) expect(k in stats, `site shows '${k}', the file lacks it`).toBe(true);
  });
});
