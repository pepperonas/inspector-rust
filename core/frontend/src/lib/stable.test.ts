import { describe, expect, it } from "vitest";
import { clipRow, keepIfEqual, Lru } from "./stable";
import type { ClipEntry } from "./types";

describe("keepIfEqual", () => {
  it("keeps the previous object for equal data", () => {
    const prev = { a: 1, list: [1, 2] };
    expect(keepIfEqual(prev, { a: 1, list: [1, 2] })).toBe(prev);
  });
  it("takes the new object when something changed", () => {
    const prev = { a: 1 };
    const next = { a: 2 };
    expect(keepIfEqual(prev, next)).toBe(next);
  });
  it("handles null on either side", () => {
    const next = { a: 1 };
    expect(keepIfEqual<{ a: number } | null>(null, next)).toBe(next);
    expect(keepIfEqual<{ a: number } | null>(next, null)).toBeNull();
  });
});

describe("clipRow", () => {
  const clip = { id: 1 } as ClipEntry;
  it("returns the same row for the same clip", () => {
    expect(clipRow(clip)).toBe(clipRow(clip));
    expect(clipRow(clip)).toEqual({ kind: "clip", data: clip });
  });
  it("a different clip object gets its own row", () => {
    expect(clipRow({ id: 1 } as ClipEntry)).not.toBe(clipRow(clip));
  });
});

describe("Lru", () => {
  it("drops the entry used longest ago", () => {
    const c = new Lru<number, string>(2);
    c.set(1, "a");
    c.set(2, "b");
    expect(c.get(1)).toBe("a"); // 1 is now the most recent
    c.set(3, "c");
    expect(c.get(2)).toBeUndefined();
    expect(c.get(1)).toBe("a");
    expect(c.get(3)).toBe("c");
    expect(c.size).toBe(2);
  });
  it("setting a key again does not grow it", () => {
    const c = new Lru<number, string>(2);
    c.set(1, "a");
    c.set(1, "b");
    expect(c.size).toBe(1);
    expect(c.get(1)).toBe("b");
  });
});
