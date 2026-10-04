import { describe, expect, it } from "vitest";
import {
  PALETTE_MAX_CELLS,
  PALETTE_MIN_CELLS,
  commitCellCount,
  validCellCount,
} from "./palette-cells";

describe("hex-grid density drafts", () => {
  it("only a complete in-range number is saved while typing", () => {
    expect(validCellCount("12")).toBe(12);
    expect(validCellCount(" 9 ")).toBe(9);
    // On the way to "12" — must not be saved (it would come back clamped).
    expect(validCellCount("1")).toBeNull();
    expect(validCellCount("")).toBeNull();
    expect(validCellCount("25")).toBeNull();
    expect(validCellCount("1.5")).toBeNull();
    expect(validCellCount("-3")).toBeNull();
  });

  it("leaving the field clamps a number and keeps the value for anything else", () => {
    expect(commitCellCount("99", 10)).toBe(PALETTE_MAX_CELLS);
    expect(commitCellCount("1", 10)).toBe(PALETTE_MIN_CELLS);
    expect(commitCellCount("", 10)).toBe(10);
    expect(commitCellCount("abc", 7)).toBe(7);
    expect(commitCellCount("14", 10)).toBe(14);
  });

  it("uses the same bounds as the backend", async () => {
    // Read from disk (no @types/node → computed specifier, see motion-stage.test).
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const rs = readFileSync(cwd + "/../rust-lib/src/window_palette/mod.rs", "utf8");
    expect(Number(rs.match(/const MIN_CELLS: u32 = (\d+);/)?.[1])).toBe(PALETTE_MIN_CELLS);
    expect(Number(rs.match(/const MAX_CELLS: u32 = (\d+);/)?.[1])).toBe(PALETTE_MAX_CELLS);
  });
});
