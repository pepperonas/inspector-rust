import { describe, it, expect } from "vitest";

/** Every .ts/.tsx under src, as source text (tests excluded). */
const SOURCES = import.meta.glob(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

describe("destructive confirmations", () => {
  it("nothing calls window.confirm (it can pass without showing a dialog — use confirmDialog)", () => {
    const files = Object.keys(SOURCES);
    expect(files.length).toBeGreaterThan(100); // the glob found the tree
    const hits = files.filter((f) => {
      const code = SOURCES[f].replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
      return /\bwindow\.confirm\s*\(/.test(code);
    });
    expect(hits).toEqual([]);
  });
});
