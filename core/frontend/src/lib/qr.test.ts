import { describe, it, expect } from "vitest";
import { qrMatrix } from "./qr";

describe("qrMatrix", () => {
  it("returns a square boolean matrix", () => {
    const m = qrMatrix("hello");
    expect(m.length).toBeGreaterThan(0);
    expect(m.every((row) => row.length === m.length)).toBe(true);
    expect(m.every((row) => row.every((cell) => typeof cell === "boolean"))).toBe(true);
  });

  it("has the three finder patterns (dark 7×7 corner squares)", () => {
    const m = qrMatrix("inspector-rust");
    const n = m.length;
    // The finder pattern's outer ring is dark at the very corner module.
    expect(m[0][0]).toBe(true); // top-left
    expect(m[0][n - 1]).toBe(true); // top-right
    expect(m[n - 1][0]).toBe(true); // bottom-left
    // The bottom-right has no finder pattern → not the same fixed dark corner.
  });

  it("is deterministic for the same input", () => {
    expect(qrMatrix("abc")).toEqual(qrMatrix("abc"));
  });

  it("grows the matrix for longer input", () => {
    const small = qrMatrix("hi").length;
    const big = qrMatrix("x".repeat(200)).length;
    expect(big).toBeGreaterThan(small);
  });

  it("throws on empty text", () => {
    expect(() => qrMatrix("")).toThrow();
  });
});
