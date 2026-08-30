import { describe, it, expect } from "vitest";
import { sleepIndicator, indicatorTicks } from "./sleep-indicator";
import type { SleepStatus } from "./ipc";

const base: SleepStatus = {
  supported: true,
  sleep_disabled: false,
  prevented: false,
  indefinite: false,
  max_timeout_secs: null,
  holders: [],
};
const s = (p: Partial<SleepStatus> = {}): SleepStatus => ({ ...base, ...p });

describe("sleepIndicator", () => {
  it("shows the wakelock even when the profile disables sleep", () => {
    // ⚠️ THE reported defect: with a stored AC profile of `sleep 0` the old
    // amber branch returned before assertions were considered, so toggling
    // the wakelock changed nothing on screen.
    const withProfileOff = s({ sleep_disabled: true });
    expect(sleepIndicator(withProfileOff, true, null)?.kind).toBe("wake");
    // ...and turning it off falls through to the profile, which is still true.
    expect(sleepIndicator(withProfileOff, false, null)?.kind).toBe("no-sleep");
  });

  it("says 'off' out loud instead of disappearing", () => {
    // The old wake LED rendered only while ON, so "off" looked like a broken
    // indicator. Nothing preventing sleep must still produce a reading.
    const off = sleepIndicator(s(), false, null);
    expect(off?.kind).toBe("sleepable");
    expect(off?.label).toBe("sleep");
  });

  it("appears immediately on a wakelock toggle, before the first poll", () => {
    // status === null = the 10 s poll has not answered yet.
    expect(sleepIndicator(null, true, null)?.kind).toBe("wake");
  });

  it("stays hidden where there is nothing truthful to say", () => {
    expect(sleepIndicator(null, false, null)).toBeNull();
    expect(sleepIndicator(s({ supported: false }), false, null)).toBeNull();
    // ⚠️ But an unsupported platform must NOT swallow the user's own wakelock.
    expect(sleepIndicator(s({ supported: false }), true, null)?.kind).toBe("wake");
  });

  it("distinguishes an endless hold from an expiring one", () => {
    expect(sleepIndicator(s({ prevented: true, indefinite: true }), false, null)?.label).toBe(
      "wach ∞",
    );
    const timed = sleepIndicator(
      s({ prevented: true, max_timeout_secs: 300 }),
      false,
      252,
    );
    expect(timed?.kind).toBe("awake-timed");
    // The LOCAL tick wins over the polled value, so the countdown moves.
    expect(timed?.label).toBe("wach 4:12");
  });

  it("falls back to the polled value when nothing has ticked yet", () => {
    expect(
      sleepIndicator(s({ prevented: true, max_timeout_secs: 300 }), false, null)?.label,
    ).toBe("wach 5:00");
  });

  it("names the holders in every reading that has them", () => {
    const held = s({ prevented: true, indefinite: true, holders: ["caffeinate ×4", "sharingd"] });
    expect(sleepIndicator(held, false, null)?.title).toContain("caffeinate ×4, sharingd");
    // Also on the wakelock reading — the user should see who else is holding.
    expect(sleepIndicator(held, true, null)?.title).toContain("sharingd");
  });

  it("names the CAUSE so the profile is not mistaken for the wakelock", () => {
    const t = sleepIndicator(s({ sleep_disabled: true }), false, null)?.title ?? "";
    expect(t).toContain("nosleep off");
    expect(t).toContain("nicht vom Wakelock");
  });
});

describe("indicatorTicks", () => {
  it("ticks only while an expiring assertion is the reason", () => {
    expect(indicatorTicks(s({ prevented: true, max_timeout_secs: 300 }), false)).toBe(true);
    // A wakelock reading has no countdown to run.
    expect(indicatorTicks(s({ prevented: true, max_timeout_secs: 300 }), true)).toBe(false);
    expect(indicatorTicks(s({ prevented: true, indefinite: true }), false)).toBe(false);
    expect(indicatorTicks(s({ sleep_disabled: true, prevented: true }), false)).toBe(false);
    expect(indicatorTicks(s(), false)).toBe(false);
    expect(indicatorTicks(null, false)).toBe(false);
  });
});
