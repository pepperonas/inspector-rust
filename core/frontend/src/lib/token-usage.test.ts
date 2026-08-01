import { describe, expect, it } from "vitest";
import {
  displayCost,
  displayTokens,
  formatActiveMin,
  formatCost,
  formatTokens,
  formatYmd,
  periodRange,
  shortProject,
} from "./token-usage";

describe("periodRange", () => {
  const today = new Date(2026, 6, 31); // local Jul 31

  it("today is a single day", () => {
    expect(periodRange("today", today)).toEqual({
      from: "2026-07-31",
      to: "2026-07-31",
    });
  });

  it("7d is inclusive of today (today − 7, matching tracker)", () => {
    expect(periodRange("7d", today)).toEqual({
      from: "2026-07-24",
      to: "2026-07-31",
    });
  });

  it("30d is inclusive (today − 30, matching tracker)", () => {
    expect(periodRange("30d", today)).toEqual({
      from: "2026-07-01",
      to: "2026-07-31",
    });
  });

  it("all omits from", () => {
    expect(periodRange("all", today)).toEqual({ from: null, to: "2026-07-31" });
  });
});

describe("formatYmd", () => {
  it("zero-pads month and day", () => {
    expect(formatYmd(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("displayTokens / displayCost", () => {
  const o = {
    input_tokens: 100,
    output_tokens: 200,
    cache_read_tokens: 600,
    cache_create_tokens: 100,
    input_cost: 1,
    output_cost: 2,
    cache_read_cost: 8,
    cache_create_cost: 1,
  };

  it("includes cache when asked", () => {
    expect(displayTokens(o, true)).toBe(1000);
    expect(displayCost(o, true)).toBe(12);
  });

  it("drops cache when toggled off", () => {
    expect(displayTokens(o, false)).toBe(300);
    expect(displayCost(o, false)).toBe(3);
  });

  it("falls back to row.cost for list rows", () => {
    expect(
      displayCost(
        {
          input_tokens: 0,
          output_tokens: 0,
          cache_read_tokens: 0,
          cache_create_tokens: 0,
          cost: 4.5,
        },
        true,
      ),
    ).toBe(4.5);
  });
});

describe("formatters", () => {
  it("formatTokens scales", () => {
    expect(formatTokens(420)).toBe("420");
    expect(formatTokens(1500)).toBe("1.5k");
    expect(formatTokens(12_300)).toBe("12k");
    expect(formatTokens(2_500_000)).toBe("2.5M");
    expect(formatTokens(1_200_000_000)).toBe("1.2B");
  });

  it("formatTokens clamps negatives and rounds", () => {
    expect(formatTokens(-5)).toBe("0");
    expect(formatTokens(999.6)).toBe("1k");
    expect(formatTokens(1_000_000)).toBe("1M");
    expect(formatTokens(1_000_000_000)).toBe("1B");
  });

  it("formatCost picks precision", () => {
    expect(formatCost(0.42)).toBe("$0.42");
    expect(formatCost(12.3)).toBe("$12.3");
    expect(formatCost(150)).toBe("$150");
  });

  it("formatCost clamps negatives", () => {
    expect(formatCost(-1)).toBe("$0.00");
    // 9.999 is still in the <10 branch; toFixed(2) rounds the display.
    expect(formatCost(9.999)).toBe("$10.00");
  });

  it("formatActiveMin", () => {
    expect(formatActiveMin(42)).toBe("42m");
    expect(formatActiveMin(120)).toBe("2h");
    expect(formatActiveMin(125)).toBe("2h 5m");
  });

  it("formatActiveMin clamps and rounds", () => {
    expect(formatActiveMin(-3)).toBe("0m");
    expect(formatActiveMin(59.6)).toBe("1h"); // rounds to 60 → 1h
    expect(formatActiveMin(61)).toBe("1h 1m");
  });

  it("shortProject keeps last two segments", () => {
    expect(shortProject("claude/inspector/rust")).toBe("inspector/rust");
    expect(shortProject("solo")).toBe("solo");
  });

  it("shortProject handles backslashes and trailing separators", () => {
    expect(shortProject("C:\\Users\\me\\proj")).toBe("me/proj");
    expect(shortProject("a/b/c/")).toBe("b/c");
    expect(shortProject("a/b")).toBe("a/b");
  });
});

describe("TOKEN_PERIODS", () => {
  it("exposes the four chips in order", async () => {
    const { TOKEN_PERIODS } = await import("./token-usage");
    expect(TOKEN_PERIODS.map((p) => p.id)).toEqual([
      "today",
      "7d",
      "30d",
      "all",
    ]);
  });
});

describe("displayTokens / displayCost edge cases", () => {
  it("sums zero fields cleanly", () => {
    const empty = {
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_create_tokens: 0,
    };
    expect(displayTokens(empty, true)).toBe(0);
    expect(displayTokens(empty, false)).toBe(0);
    expect(displayCost(empty, true)).toBe(0);
  });

  it("falls back to estimated_cost when cost is absent", () => {
    expect(
      displayCost(
        {
          input_tokens: 0,
          output_tokens: 0,
          cache_read_tokens: 0,
          cache_create_tokens: 0,
          estimated_cost: 2.25,
        },
        false,
      ),
    ).toBe(2.25);
  });

  it("periodRange crosses month / year boundaries", () => {
    expect(periodRange("7d", new Date(2026, 0, 3))).toEqual({
      from: "2025-12-27",
      to: "2026-01-03",
    });
    expect(periodRange("30d", new Date(2026, 0, 10))).toEqual({
      from: "2025-12-11",
      to: "2026-01-10",
    });
  });
});
