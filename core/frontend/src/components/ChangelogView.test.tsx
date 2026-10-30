import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ChangelogView } from "./ChangelogView";
import { CHANGELOG_FIRST, CHANGELOG_MORE } from "../lib/changelog";

describe("ChangelogView", () => {
  // Load immediately instead of waiting for the section to scroll into view.
  beforeEach(() => vi.stubGlobal("IntersectionObserver", undefined));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("shows the newest releases, marks the installed one and pages older ones", async () => {
    render(<ChangelogView currentVersion="0.185.0" />);
    const first = await screen.findAllByTestId("changelog-release", {}, { timeout: 5000 });
    expect(first).toHaveLength(CHANGELOG_FIRST);
    fireEvent.click(screen.getByRole("button", { name: "Older releases" }));
    expect(screen.getAllByTestId("changelog-release")).toHaveLength(CHANGELOG_FIRST + CHANGELOG_MORE);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "0.185.0" } });
    const hit = screen.getAllByTestId("changelog-release");
    expect(hit[0].textContent).toContain("0.185.0");
    expect(hit[0].textContent).toContain("installed");
  });

  it("renders a bullet's text as one flowing box, not one column per part", async () => {
    render(<ChangelogView />);
    const [release] = await screen.findAllByTestId("changelog-release", {}, { timeout: 5000 });
    const bullet = release.querySelector("div.flex");
    // Exactly two flex children: the marker and the text wrapper.
    expect(bullet?.childNodes).toHaveLength(2);
  });
});
