import { describe, expect, it } from "vitest";
import {
  absoluteReset,
  barFill,
  formatPercent,
  limitTone,
  limitsErrorText,
  limitsPhase,
  relativeReset,
  sinceLabel,
  standLabel,
} from "./claude-limits";

describe("claude-limits helpers", () => {
  it("clamps the bar and survives NaN", () => {
    expect(barFill(-5)).toBe(0);
    expect(barFill(42)).toBe(42);
    expect(barFill(130)).toBe(100);
    expect(barFill(NaN)).toBe(0);
  });

  it("colours by threshold", () => {
    expect(limitTone(74.9)).toBe("ok");
    expect(limitTone(75)).toBe("warn");
    expect(limitTone(90)).toBe("crit");
  });

  it("formats percent German-style", () => {
    expect(formatPercent(6)).toBe("6 %");
    expect(formatPercent(99.66)).toBe("99,7 %");
    expect(formatPercent(NaN)).toBe("–");
  });

  it("relative reset", () => {
    const now = 0;
    expect(relativeReset(30_000, now)).toBe("jetzt");
    expect(relativeReset(-1, now)).toBe("jetzt");
    expect(relativeReset(4 * 60_000, now)).toBe("in 4 Min.");
    expect(relativeReset((3 * 60 + 12) * 60_000, now)).toBe("in 3 Std. 12 Min.");
    expect(relativeReset(2 * 3600_000, now)).toBe("in 2 Std.");
    expect(relativeReset((2 * 1440 + 5 * 60) * 60_000, now)).toBe("in 2 T. 5 Std.");
  });

  it("absolute reset is in Berlin time regardless of the machine zone", () => {
    // 2026-10-10T23:00Z = Sunday 01:00 in Berlin (CEST, +2).
    const s = absoluteReset(Date.UTC(2026, 9, 10, 23, 0));
    expect(s).toContain("11.10.");
    expect(s).toContain("01:00");
    expect(standLabel(Date.UTC(2026, 0, 15, 12, 30))).toBe("Stand 13:30"); // CET, +1
  });

  it("maps error codes, including the expired-token instruction", () => {
    expect(limitsErrorText(null)).toBeNull();
    expect(limitsErrorText("limits.token_expired")).toMatch(/Claude Code einmal starten/);
    expect(limitsErrorText("limits.whatever")).toBe("limits.whatever");
  });

  it("phase", () => {
    const now = 100 * 60_000;
    expect(limitsPhase(false, null, null, 5, now)).toBe("laden");
    expect(limitsPhase(false, "limits.network", null, 5, now)).toBe("fehler");
    expect(limitsPhase(true, "limits.rate_limited", now, 5, now)).toBe("veraltet");
    expect(limitsPhase(true, null, now - 11 * 60_000, 5, now)).toBe("veraltet");
    expect(limitsPhase(true, null, now - 4 * 60_000, 5, now)).toBe("aktuell");
  });

  it("since label", () => {
    expect(sinceLabel(0, 30_000)).toBe("gerade eben");
    expect(sinceLabel(0, 12 * 60_000)).toBe("vor 12 Min.");
    expect(sinceLabel(0, 3 * 3600_000)).toBe("vor 3 Std.");
    expect(sinceLabel(0, 50 * 3600_000)).toBe("vor 2 T.");
  });
});
