import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { PlayStatusIcon, STATUS_COLOR } from "./PlayStatusIcon";

afterEach(cleanup);

const row = (status: "listened" | "partial" | "skipped" | "playing", played = 60_000, duration: number | null = 200_000) => ({
  status,
  played_ms: played,
  duration_ms: duration,
});

describe("PlayStatusIcon", () => {
  it("listened = green with the arrow running into an end stop", () => {
    render(<PlayStatusIcon row={row("listened")} />);
    const icon = screen.getByRole("img", { name: "gehört" });
    expect(icon.getAttribute("data-status")).toBe("listened");
    expect(STATUS_COLOR.listened).toBe("#10b981");
    // shaft + head + end stop
    expect(icon.querySelectorAll("path")).toHaveLength(3);
  });

  it("partial = orange tilde; the ring shows the heard share and the label says it", () => {
    render(<PlayStatusIcon row={row("partial", 50_000, 200_000)} />);
    const icon = screen.getByRole("img", { name: "angespielt · 25 %" });
    expect(STATUS_COLOR.partial).toBe("#f97316");
    expect(screen.getByTestId("play-status-progress").getAttribute("stroke-dasharray")).toBe("0.25 1");
    expect(icon.querySelector("path")?.getAttribute("d")).toMatch(/c/); // the tilde curve
  });

  it("an unknown length draws no progress instead of a guess", () => {
    render(<PlayStatusIcon row={row("partial", 50_000, null)} />);
    expect(screen.getByRole("img", { name: "angespielt" })).toBeTruthy();
    expect(screen.queryByTestId("play-status-progress")).toBeNull();
  });

  it("skipped and playing have their own shapes", () => {
    render(<PlayStatusIcon row={row("skipped")} />);
    render(<PlayStatusIcon row={row("playing")} />);
    expect(screen.getByRole("img", { name: "übersprungen" }).querySelectorAll("path")).toHaveLength(2);
    expect(screen.getByRole("img", { name: "läuft" }).querySelectorAll(".eq-bar")).toHaveLength(3);
  });
});
