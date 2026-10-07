import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
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
  it("has no item counter and no Paste/Close/Navigate hints (v0.203.0)", () => {
    render(<Footer version="1.0.0" />);
    for (const gone of ["Paste", "Close", "Navigate", "⏎", "Esc", "↑↓"]) {
      expect(screen.queryByText(gone)).toBeNull();
    }
    expect(screen.queryByText(/^\d+\/\d+$/)).toBeNull();
    expect(screen.getByText("OCR")).toBeTruthy();
  });

  it("top row: free space on the right; bottom row: folder left, version right", () => {
    render(
      <Footer
        version="0.203.0"
        freeSpace={{ available: 16e9, total: 500e9, name: "Macintosh HD", mount: "/" }}
        finderContext={{ dir: "/Users/m/Downloads", selected: [], selected_count: 0, from_selection: false }}
      />,
    );
    const top = screen.getByTestId("footer-row-top");
    const bottom = screen.getByTestId("footer-row-bottom");
    expect(top.lastElementChild).toBe(screen.getByTestId("free-space"));
    expect(bottom.firstElementChild?.contains(screen.getByTestId("finder-context"))).toBe(true);
    expect(bottom.lastElementChild?.textContent).toBe("v0.203.0");
    expect(top.textContent).not.toContain("v0.203.0");
  });

  it("renders the version chip when version is provided", () => {
    render(<Footer version="0.2.6" />);
    expect(screen.getByText("v0.2.6")).toBeTruthy();
  });

  it("shows the version as the LAST element — bottom right of the footer", () => {
    const { container } = render(<Footer version="0.185.0" />);
    const chip = screen.getByText("v0.185.0");
    // Every text-bearing leaf of the footer, in document order.
    const leaves = [...container.querySelectorAll("span")].filter(
      (el) => el.children.length === 0 && el.textContent?.trim(),
    );
    expect(leaves[leaves.length - 1]).toBe(chip);
  });

  it("omits the version chip when version is undefined", () => {
    render(<Footer />);
    expect(screen.queryByText(/^v\d/)).toBeNull();
  });

  it("does not render the author credit (moved to the inline About)", () => {
    render(<Footer />);
    expect(screen.queryByText(/Martin Pfeffer/)).toBeNull();
  });

  it("hides the wakelock LED by default (wakelockActive omitted)", () => {
    render(<Footer />);
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("hides the wakelock LED when wakelockActive=false", () => {
    render(<Footer wakelockActive={false} />);
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("shows the wakelock LED + label when wakelockActive=true", () => {
    render(<Footer wakelockActive={true} />);
    // The LED label is `wake` next to the red dot — easy text probe.
    expect(screen.getByText("wake")).toBeTruthy();
  });
});

describe("Footer — system sleep indicator", () => {
  it("is hidden only where there is nothing truthful to say", () => {
    const probe = () => screen.queryByText(/no-sleep|wach|^wake$|^sleep$/);
    // No status yet + no wakelock -> nothing is known.
    render(<Footer />);
    expect(probe()).toBeNull();
    cleanup();
    // Unsupported platform -> nothing to report.
    render(<Footer sleepStatus={sleep({ supported: false, prevented: true, indefinite: true })} />);
    expect(probe()).toBeNull();
    cleanup();
    // ⚠️ But an unsupported status must not swallow the user's OWN wakelock.
    render(<Footer wakelockActive={true} sleepStatus={sleep({ supported: false })} />);
    expect(screen.getByText("wake")).toBeTruthy();
  });

  it("shows amber no-sleep when the active profile disables sleep — even with holders", () => {
    // sleep 0 makes the countdown a lie (sleep never happens), so it wins.
    render(
      <Footer
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
      <Footer sleepStatus={sleep({ prevented: true, indefinite: true, holders: ["sharingd"] })} />,
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
    render(<Footer wakelockActive={true} sleepStatus={st} />);
    expect(screen.getByText("wake")).toBeTruthy();
    expect(screen.queryByText("no-sleep")).toBeNull();
    cleanup();
    render(<Footer wakelockActive={false} sleepStatus={st} />);
    expect(screen.getByText("no-sleep")).toBeTruthy();
    expect(screen.queryByText("wake")).toBeNull();
  });

  it("says 'sleep' out loud instead of vanishing when nothing holds the Mac", () => {
    // The old wake LED rendered only while ON, so "off" was indistinguishable
    // from a broken indicator.
    render(<Footer wakelockActive={false} sleepStatus={sleep({})} />);
    expect(screen.getByText("sleep")).toBeTruthy();
  });
});

describe("Footer — dark-wake toggle", () => {
  it("is hidden without a handler (cold mounts stay clean)", () => {
    const { container } = render(<Footer />);
    expect(container.querySelector("button")).toBeNull();
  });

  it("renders muted without the srv label while off, and toggles on click", () => {
    const onToggle = vi.fn();
    const { container } = render(
      <Footer darkWake={false} onDarkWakeToggle={onToggle} />,
    );
    const btn = container.querySelector("button")!;
    expect(btn).toBeTruthy();
    expect(screen.queryByText("srv")).toBeNull();
    btn.click();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("shows the violet srv badge while dark wake is on", () => {
    render(<Footer darkWake={true} onDarkWakeToggle={() => {}} />);
    expect(screen.getByText("srv")).toBeTruthy();
  });

  it("is labelled in BOTH states — an unlabelled glyph is how it got lost", () => {
    // ⚠️ Regression pin (v0.155.0). While off this was a bare 11 px moon with
    // no text, next to labelled glowing indicators; the user reported it as
    // simply gone. Every other footer item is glyph + uppercase mono label.
    render(<Footer darkWake={false} onDarkWakeToggle={() => {}} />);
    expect(screen.getByText("dark")).toBeTruthy();
    cleanup();
    render(<Footer darkWake={true} onDarkWakeToggle={() => {}} />);
    expect(screen.getByText("srv")).toBeTruthy();
  });

  it("does not dim itself out of sight while resting", () => {
    // ⚠️ `opacity-60` on an already-muted 11 px glyph is what made it
    // invisible. It is a control and must read as one.
    const { container } = render(
      <Footer darkWake={false} onDarkWakeToggle={() => {}} />,
    );
    const btn = container.querySelector("button")!;
    expect(btn.className).not.toContain("opacity-60");
  });

  it("comes BEFORE the sleep indicator, as it did until v0.152.0", () => {
    // ⚠️ Merging the two indicators put an always-visible badge in front of
    // this button; with the wakelock off it used to be the leftmost item.
    const { container } = render(
      <Footer
        darkWake={false}
        onDarkWakeToggle={() => {}}
        sleepStatus={sleep({ sleep_disabled: true })}
      />,
    );
    const btn = container.querySelector("button")!;
    const badge = screen.getByText("no-sleep");
    // DOCUMENT_POSITION_FOLLOWING === 4: the badge follows the button.
    expect(btn.compareDocumentPosition(badge) & 4).toBeTruthy();
  });

  it("coexists with the full wakelock LED (two modes, one backend)", () => {
    // App never sets both, but the footer must not couple them structurally.
    render(
      <Footer
        wakelockActive={true}
        darkWake={false}
        onDarkWakeToggle={() => {}}
      />,
    );
    expect(screen.getByText("wake")).toBeTruthy();
    expect(screen.queryByText("srv")).toBeNull();
  });
});

describe("Footer free space (v0.195.0)", () => {
  const T = 494_384_795_648;
  it("always shows the free space with System Settings' tooltip", () => {
    render(<Footer freeSpace={{ name: "Macintosh HD", mount: "/", available: 186_000_000_000, total: T }} />);
    const el = screen.getByTestId("free-space");
    expect(el.textContent).toContain("186 GB frei");
    expect(el.getAttribute("title")).toBe("Macintosh HD — 186,00 GB verfügbar von 494,38 GB");
    expect(el.dataset.level).toBe("ok");
  });
  it("turns red when the disk is nearly full", () => {
    render(<Footer freeSpace={{ name: "Macintosh HD", mount: "/", available: 3_900_000_000, total: T }} />);
    const el = screen.getByTestId("free-space");
    expect(el.textContent).toContain("3,9 GB frei");
    expect(el.dataset.level).toBe("crit");
    expect(el.className).toContain("text-red-500");
  });
  it("renders nothing until the first reading arrives", () => {
    render(<Footer freeSpace={null} />);
    expect(screen.queryByTestId("free-space")).toBeNull();
  });

  describe("finder working folder", () => {
    const ctx = {
      dir: "/Users/martin/claude/inspector-rust/docs",
      selected: ["/Users/martin/claude/inspector-rust/docs/notiz.md"],
      selected_count: 1,
      from_selection: true,
    };

    it("shows the short folder and the selected file; click reveals the folder", () => {
      const onReveal = vi.fn();
      render(<Footer finderContext={ctx} onRevealFolder={onReveal} />);
      const chip = screen.getByTestId("finder-context");
      expect(chip.textContent).toContain("…/inspector-rust/docs");
      expect(chip.textContent).toContain("notiz.md");
      expect(chip.getAttribute("title")).toContain("~/claude/inspector-rust/docs");
      fireEvent.click(chip);
      expect(onReveal).toHaveBeenCalledWith(ctx.dir);
    });

    it("is hidden without a context", () => {
      render(<Footer />);
      expect(screen.queryByTestId("finder-context")).toBeNull();
    });
  });
});
