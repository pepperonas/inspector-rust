import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import { Footer } from "./Footer";
import type { SleepStatus } from "../lib/ipc";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** A SleepStatus with sane defaults, overridable per test. */
function sleep(over: Partial<SleepStatus>): SleepStatus {
  return {
    supported: true,
    sleep_disabled: false,
    prevented: false,
    indefinite: false,
    max_timeout_secs: null,
    holders: [],
    ...over,
  };
}

describe("Footer", () => {
  it("shows 1-based counter label", () => {
    render(<Footer index={0} total={5} />);
    expect(screen.getByText("1/5")).toBeTruthy();
  });

  it('shows "0/0" when there are no entries', () => {
    render(<Footer index={0} total={0} />);
    expect(screen.getByText("0/0")).toBeTruthy();
  });

  it("shows the correct counter for the last entry", () => {
    render(<Footer index={9} total={10} />);
    expect(screen.getByText("10/10")).toBeTruthy();
  });

  it("renders all keyboard hint labels", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.getByText("Paste")).toBeTruthy();
    expect(screen.getByText("Navigate")).toBeTruthy();
    expect(screen.getByText("Close")).toBeTruthy();
  });

  it("renders keyboard shortcut keys", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.getByText("⏎")).toBeTruthy();
    expect(screen.getByText("↑↓")).toBeTruthy();
    expect(screen.getByText("Esc")).toBeTruthy();
  });

  it("renders the version chip when version is provided", () => {
    render(<Footer index={0} total={1} version="0.2.6" />);
    expect(screen.getByText("v0.2.6")).toBeTruthy();
  });

  it("omits the version chip when version is undefined", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.queryByText(/^v\d/)).toBeNull();
  });

  it("does not render the author credit (moved to the inline About)", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.queryByText(/Martin Pfeffer/)).toBeNull();
  });

  it("hides the wakelock LED by default (wakelockActive omitted)", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("hides the wakelock LED when wakelockActive=false", () => {
    render(<Footer index={0} total={1} wakelockActive={false} />);
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("shows the wakelock LED + label when wakelockActive=true", () => {
    render(<Footer index={0} total={1} wakelockActive={true} />);
    // The LED label is `wake` next to the red dot — easy text probe.
    expect(screen.getByText("wake")).toBeTruthy();
  });
});

describe("Footer — system sleep indicator", () => {
  it("is hidden only where there is nothing truthful to say", () => {
    const probe = () => screen.queryByText(/no-sleep|wach|^wake$|^sleep$/);
    // No status yet + no wakelock -> nothing is known.
    render(<Footer index={0} total={1} />);
    expect(probe()).toBeNull();
    cleanup();
    // Unsupported platform -> nothing to report.
    render(<Footer index={0} total={1} sleepStatus={sleep({ supported: false, prevented: true, indefinite: true })} />);
    expect(probe()).toBeNull();
    cleanup();
    // ⚠️ But an unsupported status must not swallow the user's OWN wakelock.
    render(<Footer index={0} total={1} wakelockActive={true} sleepStatus={sleep({ supported: false })} />);
    expect(screen.getByText("wake")).toBeTruthy();
  });

  it("shows amber no-sleep when the active profile disables sleep — even with holders", () => {
    // sleep 0 makes the countdown a lie (sleep never happens), so it wins.
    render(
      <Footer
        index={0}
        total={1}
        sleepStatus={sleep({ sleep_disabled: true, prevented: true, max_timeout_secs: 300, holders: ["caffeinate ×4"] })}
      />,
    );
    expect(screen.getByText("no-sleep")).toBeTruthy();
    expect(screen.queryByText(/^wach/)).toBeNull();
  });

  it("shows a ticking countdown for a timed prevention and parks at 0:00", () => {
    vi.useFakeTimers();
    render(
      <Footer
        index={0}
        total={1}
        sleepStatus={sleep({ prevented: true, max_timeout_secs: 252, holders: ["caffeinate ×4", "sharingd"] })}
      />,
    );
    expect(screen.getByText("wach 4:12")).toBeTruthy();
    // The tooltip names the holders.
    expect(screen.getByTitle(/caffeinate ×4, sharingd/)).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByText("wach 4:10")).toBeTruthy();
    // Long past expiry: parked at 0:00, never negative (next poll corrects).
    act(() => {
      vi.advanceTimersByTime(600_000);
    });
    expect(screen.getByText("wach 0:00")).toBeTruthy();
  });

  it("shows ∞ for an indefinite prevention", () => {
    render(
      <Footer index={0} total={1} sleepStatus={sleep({ prevented: true, indefinite: true, holders: ["sharingd"] })} />,
    );
    expect(screen.getByText("wach ∞")).toBeTruthy();
    expect(screen.getByTitle(/sharingd/)).toBeTruthy();
  });

  it("shows ONE reading, not two competing ones (v0.152.0)", () => {
    // ⚠️ Deliberate change from v0.114.0, which rendered the wake LED and the
    // system badge side by side. Two indicators answering "will it sleep?"
    // contradicted each other in the field: the amber profile badge could not
    // react to the wakelock at all, so toggling it appeared to do nothing.
    render(
      <Footer
        index={0}
        total={1}
        wakelockActive={true}
        sleepStatus={sleep({ prevented: true, indefinite: true, holders: ["caffeinate"] })}
      />,
    );
    expect(screen.getByText("wake")).toBeTruthy();
    expect(screen.queryByText("wach ∞")).toBeNull();
  });

  it("lets the wakelock outrank a sleep-disabled profile, and falls back when it is off", () => {
    // THE reported defect: with a stored AC profile of `sleep 0` the old amber
    // branch returned before assertions were considered.
    const st = sleep({ sleep_disabled: true, prevented: true, holders: ["caffeinate ×4"] });
    render(<Footer index={0} total={1} wakelockActive={true} sleepStatus={st} />);
    expect(screen.getByText("wake")).toBeTruthy();
    expect(screen.queryByText("no-sleep")).toBeNull();
    cleanup();
    render(<Footer index={0} total={1} wakelockActive={false} sleepStatus={st} />);
    expect(screen.getByText("no-sleep")).toBeTruthy();
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("says 'sleep' out loud instead of vanishing when nothing holds the Mac", () => {
    // The old wake LED rendered only while ON, so "off" was indistinguishable
    // from a broken indicator.
    render(<Footer index={0} total={1} wakelockActive={false} sleepStatus={sleep({})} />);
    expect(screen.getByText("sleep")).toBeTruthy();
  });
});

describe("Footer — dark-wake toggle", () => {
  it("is hidden without a handler (cold mounts stay clean)", () => {
    const { container } = render(<Footer index={0} total={1} />);
    expect(container.querySelector("button")).toBeNull();
  });

  it("renders muted without the srv label while off, and toggles on click", () => {
    const onToggle = vi.fn();
    const { container } = render(
      <Footer index={0} total={1} darkWake={false} onDarkWakeToggle={onToggle} />,
    );
    const btn = container.querySelector("button")!;
    expect(btn).toBeTruthy();
    expect(screen.queryByText("srv")).toBeNull();
    btn.click();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("shows the violet srv badge while dark wake is on", () => {
    render(<Footer index={0} total={1} darkWake={true} onDarkWakeToggle={() => {}} />);
    expect(screen.getByText("srv")).toBeTruthy();
  });

  it("coexists with the full wakelock LED (two modes, one backend)", () => {
    // App never sets both, but the footer must not couple them structurally.
    render(
      <Footer
        index={0}
        total={1}
        wakelockActive={true}
        darkWake={false}
        onDarkWakeToggle={() => {}}
      />,
    );
    expect(screen.getByText("wake")).toBeTruthy();
    expect(screen.queryByText("srv")).toBeNull();
  });
});
