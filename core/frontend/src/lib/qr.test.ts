import { describe, it, expect } from "vitest";
import { qrMatrix, drawQr } from "./qr";

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

  it("uses the smallest version (21×21) for short text", () => {
    // QR version 1 is 21 modules; auto-fit must not over-size a tiny payload.
    expect(qrMatrix("hi").length).toBe(21);
  });

  it("matrix size is always a valid QR version size (17 + 4·version)", () => {
    for (const text of ["a", "hello world", "x".repeat(100), "y".repeat(400)]) {
      const n = qrMatrix(text).length;
      expect(n).toBeGreaterThanOrEqual(21);
      expect((n - 17) % 4).toBe(0);
    }
  });

  it("contains the horizontal timing pattern (row 6 alternates)", () => {
    const m = qrMatrix("timing");
    const n = m.length;
    // Between the finder patterns (cols 8 .. n-9) row 6 alternates dark/light,
    // dark on even columns — a structural QR invariant any scanner relies on.
    for (let c = 8; c <= n - 9; c++) {
      expect(m[6][c]).toBe(c % 2 === 0);
    }
  });

  it("finder pattern has the dark-ring / light-separator / dark-core structure", () => {
    const m = qrMatrix("finder");
    expect(m[0][0]).toBe(true); // outer ring
    expect(m[1][1]).toBe(false); // inner light ring
    expect(m[3][3]).toBe(true); // dark 3×3 core centre
    expect(m[0][7]).toBe(false); // separator column right of the finder
    expect(m[7][0]).toBe(false); // separator row below the finder
  });

  it("different inputs produce different matrices", () => {
    expect(qrMatrix("aaaa")).not.toEqual(qrMatrix("bbbb"));
  });

  it("accepts Umlaut / non-ASCII input without throwing", () => {
    const m = qrMatrix("Grüße aus München");
    expect(m.length).toBeGreaterThanOrEqual(21);
    expect(m.every((row) => row.length === m.length)).toBe(true);
    // The encoded payload differs from the plain-ASCII spelling.
    expect(m).not.toEqual(qrMatrix("Gruesse aus Muenchen"));
  });
});

describe("drawQr", () => {
  it("sizes the canvas to modules + quiet zone at the given scale", () => {
    const canvas = document.createElement("canvas");
    const n = qrMatrix("hi").length; // 21
    drawQr(canvas, "hi", 6, 4);
    expect(canvas.width).toBe((n + 8) * 6);
    expect(canvas.height).toBe(canvas.width);
  });

  it("honours custom scale and margin", () => {
    const canvas = document.createElement("canvas");
    const n = qrMatrix("hi").length;
    drawQr(canvas, "hi", 2, 0);
    expect(canvas.width).toBe(n * 2);
    drawQr(canvas, "hi", 10, 1);
    expect(canvas.width).toBe((n + 2) * 10);
  });
});
