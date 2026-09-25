// The product page (inspector-rust.celox.io) shows EVERY feature by mirroring features.txt: the
// server's release timer fetches the file from GitHub every 15 minutes and renders it (template
// `feature_catalog`, parser `parse_catalog` in website/server/inspector-rust-latest.py). A new feature
// therefore reaches the website by the same one-line entry the doc contract already demands — as
// long as the line is in a shape that parser understands. These tests pin that shape, mirroring its
// rules: lines before the first "== Area ==" are dropped, and a feature is "Name — description"
// with the separator within the first 80 characters.
import { describe, expect, it } from "vitest";
import featuresTxt from "../../../../features.txt?raw";
import siteJson from "../../../../website/site.json?raw";

const SECTION = /^==\s*(.+?)\s*==\s*$/;
const SEPARATORS = [" — ", " – ", " - "];
const NAME_MAX = 80;

function catalog(text: string) {
  const lines = text.replace(/\r/g, "").split("\n").map((l) => l.trim());
  const first = lines.findIndex((l) => SECTION.test(l));
  const before = lines.slice(0, Math.max(first, 0)).filter(Boolean);
  const areas: { area: string; items: string[] }[] = [];
  for (const l of lines.slice(first)) {
    const m = SECTION.exec(l);
    if (m) areas.push({ area: m[1], items: [] });
    else if (l && areas.length) areas[areas.length - 1].items.push(l);
  }
  return { first, before, areas };
}

function hasName(line: string) {
  return SEPARATORS.some((sep) => {
    const i = line.indexOf(sep);
    return i > 0 && i <= NAME_MAX;
  });
}

describe("website feature catalogue (features.txt → product page)", () => {
  const { first, before, areas } = catalog(featuresTxt);

  it("the site is wired to features.txt", () => {
    const site = JSON.parse(siteJson) as { feature_catalog?: string };
    expect(site.feature_catalog).toBe("features.txt");
  });

  it("starts with an area heading — anything above it never reaches the site", () => {
    expect(first, 'features.txt needs "== Area ==" headings').toBeGreaterThanOrEqual(0);
    // Only the title line may precede the first area.
    expect(before.length, `lines above the first "== Area ==" are dropped: ${before.slice(1).join(" | ")}`).toBeLessThanOrEqual(1);
  });

  it("has no empty areas", () => {
    expect(areas.filter((a) => a.items.length === 0).map((a) => a.area)).toEqual([]);
  });

  it('every feature (outside "Plattformen") reads "Name — description"', () => {
    const unnamed = areas
      .filter((a) => a.area !== "Plattformen")
      .flatMap((a) => a.items.filter((l) => !hasName(l)).map((l) => `${a.area}: ${l.slice(0, 60)}`));
    expect(unnamed, "these would show on the website without a bold name").toEqual([]);
  });

  it("the parser rules above are not vacuous", () => {
    // A guard that finds nothing proves nothing: the real file must yield real content.
    expect(areas.length).toBeGreaterThanOrEqual(10);
    expect(areas.reduce((n, a) => n + a.items.length, 0)).toBeGreaterThanOrEqual(150);
    expect(hasName("Pins — keep clips")).toBe(true);
    expect(hasName(`${"x".repeat(90)} — too long to be a name`)).toBe(false);
    expect(catalog("stray — line\n== A ==\nx — y").before).toEqual(["stray — line"]);
  });
});
