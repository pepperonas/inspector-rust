import { describe, it, expect } from "vitest";
import { formatSleepCountdown, formatHolders } from "./sleep-status";

describe("formatSleepCountdown", () => {
  it("formats minutes:seconds below an hour", () => {
    expect(formatSleepCountdown(252)).toBe("4:12");
    expect(formatSleepCountdown(59)).toBe("0:59");
    expect(formatSleepCountdown(600)).toBe("10:00");
  });

  it("switches to h:mm:ss from one hour", () => {
    expect(formatSleepCountdown(3600)).toBe("1:00:00");
    expect(formatSleepCountdown(3661)).toBe("1:01:01");
    expect(formatSleepCountdown(7325)).toBe("2:02:05");
  });

  it("clamps to zero and never goes negative", () => {
    // The footer ticks locally between polls; at expiry it must PARK at 0:00
    // until the next poll corrects it, not run into negative time.
    expect(formatSleepCountdown(0)).toBe("0:00");
    expect(formatSleepCountdown(-5)).toBe("0:00");
  });

  it("floors fractional seconds", () => {
    expect(formatSleepCountdown(61.9)).toBe("1:01");
  });
});

describe("formatHolders", () => {
  it("joins the backend's pre-counted names", () => {
    expect(formatHolders(["caffeinate ×4", "sharingd"])).toBe("caffeinate ×4, sharingd");
    expect(formatHolders([])).toBe("");
  });
});
