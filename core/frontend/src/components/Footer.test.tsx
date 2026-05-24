import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { Footer } from "./Footer";

afterEach(cleanup);

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

  it("renders the author credit", () => {
    render(<Footer index={0} total={1} />);
    expect(screen.getByText(/Martin Pfeffer/)).toBeTruthy();
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
