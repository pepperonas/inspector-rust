import { describe, it, expect } from "vitest";

/**
 * A raw control byte in a source file is invisible in every editor and turns
 * the file BINARY for the tooling that reads it:
 *
 *   - `file` reports it as `data`, not source.
 *   - grep/ugrep SKIP it — silently. No error, no output, no exit code that
 *     says "I ignored a file". A recursive search simply never mentions it.
 *
 * That last part is why this guard exists rather than a lint rule nobody
 * reads. A single NUL in `SettingsPanel.tsx` (a separator in a template
 * literal, written as the byte instead of the escape) made every search of
 * that 219 KB file come back empty, and twice produced the confident, wrong
 * conclusion that code living in it did not exist. The defect cannot be seen,
 * only measured — so it is measured here.
 *
 * The fix is never to change the VALUE: a NUL separator is a fine choice
 * because it cannot occur in the inputs. Write it as a Unicode escape, which
 * is the identical string at runtime and plain ASCII on disk.
 */

const FRONTEND = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const RUST = import.meta.glob("../../../rust-lib/src/**/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const SOURCES = { ...FRONTEND, ...RUST };

/** Tab, newline and carriage return are the only control characters source may hold. */
const isForbidden = (code: number) =>
  (code < 0x09 || code === 0x0b || code === 0x0c || (code >= 0x0e && code <= 0x1f) || code === 0x7f);

type Hit = { file: string; line: number; column: number; code: string };

const scan = (only?: (code: number) => boolean): Hit[] => {
  const hits: Hit[] = [];
  for (const [file, text] of Object.entries(SOURCES)) {
    let line = 1;
    let lineStart = 0;
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      if (code === 0x0a) {
        line++;
        lineStart = i + 1;
        continue;
      }
      if (isForbidden(code) && (!only || only(code))) {
        hits.push({
          file,
          line,
          column: i - lineStart + 1,
          code: `0x${code.toString(16).padStart(2, "0")}`,
        });
      }
    }
  }
  return hits;
};

const show = (hits: Hit[]) =>
  hits.map((h) => `${h.file}:${h.line}:${h.column} (${h.code})`).join("\n");

describe("source files stay searchable", () => {
  // An empty glob would make every assertion below true by vacuity — the
  // classic green-blind test. Pin that it actually found the tree.
  it("the globs actually reach both source trees", () => {
    expect(Object.keys(FRONTEND).length).toBeGreaterThan(100);
    expect(Object.keys(RUST).length).toBeGreaterThan(50);
  });

  it("no source file contains a NUL byte", () => {
    const hits = scan((code) => code === 0x00);
    expect(
      hits.length,
      hits.length
        ? `NUL byte makes these files binary — grep skips them silently.\n` +
          `Write the separator as the escape ${"\\"}u0000 instead:\n${show(hits)}`
        : "",
    ).toBe(0);
  });

  it("no source file contains other invisible control characters", () => {
    const hits = scan((code) => code !== 0x00);
    expect(
      hits.length,
      hits.length ? `Invisible control characters in source:\n${show(hits)}` : "",
    ).toBe(0);
  });
});
