import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { formatAbsolute, formatBytes, truncateOneLine, relativeTime } from "./format";

describe("formatBytes", () => {
  it("formats zero bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("formats bytes below 1 KB", () => {
    expect(formatBytes(500)).toBe("500 B");
  });

  it("formats kilobytes", () => {
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("formats exactly 1 KB", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
  });

  it("formats megabytes", () => {
    expect(formatBytes(1_048_576)).toBe("1.0 MB");
  });

  it("formats fractional megabytes", () => {
    expect(formatBytes(2_621_440)).toBe("2.5 MB");
  });
});

describe("truncateOneLine", () => {
  it("returns short text unchanged", () => {
    expect(truncateOneLine("hello")).toBe("hello");
  });

  it("collapses internal newlines to spaces", () => {
    expect(truncateOneLine("hello\nworld")).toBe("hello world");
  });

  it("collapses multiple whitespace characters", () => {
    expect(truncateOneLine("a   \t  b")).toBe("a b");
  });

  it("trims leading and trailing whitespace", () => {
    expect(truncateOneLine("  hello  ")).toBe("hello");
  });

  it("does not truncate text at the exact limit", () => {
    const exact = "a".repeat(120);
    expect(truncateOneLine(exact, 120)).toBe(exact);
  });

  it("truncates text exceeding the limit and appends ellipsis", () => {
    const long = "a".repeat(200);
    const result = truncateOneLine(long, 120);
    expect(result).toHaveLength(120);
    expect(result.endsWith("…")).toBe(true);
  });

  it("respects a custom max length", () => {
    const result = truncateOneLine("hello world", 5);
    expect(result).toHaveLength(5);
    expect(result.endsWith("…")).toBe(true);
  });
});

describe("relativeTime", () => {
  const FIXED_NOW = new Date("2026-01-15T12:00:00.000Z").getTime();

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns "just now" for a timestamp under 60 seconds ago', () => {
    expect(relativeTime(FIXED_NOW - 30_000)).toBe("just now");
  });

  it('returns "just now" for a timestamp at exactly 0 ms ago', () => {
    expect(relativeTime(FIXED_NOW)).toBe("just now");
  });

  it("returns minutes for a timestamp under 1 hour ago", () => {
    expect(relativeTime(FIXED_NOW - 5 * 60_000)).toBe("5m ago");
    expect(relativeTime(FIXED_NOW - 59 * 60_000)).toBe("59m ago");
  });

  it("returns hours for a timestamp under 1 day ago", () => {
    expect(relativeTime(FIXED_NOW - 3 * 3_600_000)).toBe("3h ago");
    expect(relativeTime(FIXED_NOW - 23 * 3_600_000)).toBe("23h ago");
  });

  it("returns days for a timestamp under 1 week ago", () => {
    expect(relativeTime(FIXED_NOW - 2 * 86_400_000)).toBe("2d ago");
    expect(relativeTime(FIXED_NOW - 6 * 86_400_000)).toBe("6d ago");
  });

  it("returns a locale date string for timestamps older than 1 week", () => {
    const result = relativeTime(FIXED_NOW - 10 * 86_400_000);
    expect(result.length).toBeGreaterThan(0);
    expect(result).not.toMatch(/ago$/);
  });

  it("crosses each threshold at the exact boundary (inclusive lower bucket)", () => {
    // Boundaries are `<` comparisons, so exactly 60_000 ms falls into the NEXT
    // bucket. Pin those transitions so a future off-by-one is caught.
    expect(relativeTime(FIXED_NOW - 59_999)).toBe("just now");
    expect(relativeTime(FIXED_NOW - 60_000)).toBe("1m ago");
    expect(relativeTime(FIXED_NOW - 3_600_000)).toBe("1h ago");
    expect(relativeTime(FIXED_NOW - 86_400_000)).toBe("1d ago");
    // Exactly one week → falls out of the "d ago" bucket into the date branch.
    expect(relativeTime(FIXED_NOW - 604_800_000)).not.toMatch(/ago$/);
  });

  it("does not crash on a future timestamp (negative diff → 'just now')", () => {
    expect(relativeTime(FIXED_NOW + 5_000)).toBe("just now");
  });
});

describe("formatAbsolute", () => {
  it("renders a non-empty human string containing the year", () => {
    const s = formatAbsolute(new Date("2026-06-14T09:30:00Z").getTime());
    expect(s.length).toBeGreaterThan(0);
    expect(s).toContain("2026");
  });

  it("produces different strings for timestamps in different years", () => {
    const a = formatAbsolute(new Date("2020-01-01T00:00:00Z").getTime());
    const b = formatAbsolute(new Date("2026-01-01T00:00:00Z").getTime());
    expect(a).not.toBe(b);
    expect(a).toContain("2020");
    expect(b).toContain("2026");
  });

  it("does not throw on the unix epoch (0) or a negative timestamp", () => {
    expect(() => formatAbsolute(0)).not.toThrow();
    expect(() => formatAbsolute(-1000)).not.toThrow();
  });
});
