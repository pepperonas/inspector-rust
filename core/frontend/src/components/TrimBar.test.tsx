import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor, screen } from "@testing-library/react";

// The bar's maths lives in the pure lib/trim-range.ts (its own suite). These
// tests cover what only the COMPONENT does: fetch the proxy once per URL,
// survive a proxy failure honestly, and route the transport buttons to the
// right places in the range.
const socialAudioProxy = vi.fn<(url: string) => Promise<string>>();
vi.mock("../lib/ipc", () => ({
  socialAudioProxy: (u: string) => socialAudioProxy(u),
}));
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}));

import { TrimBar } from "./TrimBar";
import { fullRange } from "../lib/trim-range";

const URL = "https://youtu.be/os0PDw-bwrY";
const DUR = 4956;

beforeEach(() => {
  socialAudioProxy.mockReset();
  socialAudioProxy.mockResolvedValue("/cache/proxy-abc.m4a");
});
afterEach(() => cleanup());

describe("TrimBar", () => {
  it("fetches the scrubbing proxy exactly once and plays it via the asset protocol", async () => {
    const { container } = render(
      <TrimBar url={URL} duration={DUR} range={fullRange(DUR)} onRange={() => {}} />,
    );
    await waitFor(() => {
      expect(container.querySelector("audio")).not.toBeNull();
    });
    expect(socialAudioProxy).toHaveBeenCalledTimes(1);
    expect(socialAudioProxy).toHaveBeenCalledWith(URL);
    // ⚠️ The webview cannot play a raw filesystem path — it must go through
    // convertFileSrc (the asset protocol; the cache dir is in its scope).
    expect(container.querySelector("audio")!.getAttribute("src")).toBe(
      "asset:///cache/proxy-abc.m4a",
    );
  });

  it("reports a failed proxy instead of pretending", async () => {
    socialAudioProxy.mockRejectedValue(new Error("network down"));
    render(<TrimBar url={URL} duration={DUR} range={fullRange(DUR)} onRange={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText("Vorschau nicht ladbar")).toBeTruthy();
    });
    // No audio element — a dead player would look like a broken feature.
    expect(document.querySelector("audio")).toBeNull();
  });

  it("names the missing tool when yt-dlp is absent", async () => {
    socialAudioProxy.mockRejectedValue(new Error("no_ytdlp"));
    render(<TrimBar url={URL} duration={DUR} range={fullRange(DUR)} onRange={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText("yt-dlp fehlt")).toBeTruthy();
    });
  });

  it("'Ganzes Video' resets the range to the full duration", async () => {
    const onRange = vi.fn();
    render(
      <TrimBar url={URL} duration={DUR} range={{ start: 600, end: 620 }} onRange={onRange} />,
    );
    fireEvent.click(screen.getByText("Ganzes Video"));
    expect(onRange).toHaveBeenCalledWith(fullRange(DUR));
  });

  it("committing a typed start time moves the handle — and garbage does not", async () => {
    const onRange = vi.fn();
    render(
      <TrimBar url={URL} duration={DUR} range={{ start: 600, end: 1200 }} onRange={onRange} />,
    );
    const start = screen.getByLabelText("Start") as HTMLInputElement;
    fireEvent.focus(start);
    fireEvent.change(start, { target: { value: "12:00" } });
    fireEvent.blur(start);
    expect(onRange).toHaveBeenCalledWith({ start: 720, end: 1200 });
    onRange.mockClear();
    // ⚠️ An unreadable value must restore the display and move NOTHING —
    // otherwise a half-typed field snaps the handle to 0 mid-edit.
    fireEvent.focus(start);
    fireEvent.change(start, { target: { value: "abc" } });
    fireEvent.blur(start);
    expect(onRange).not.toHaveBeenCalled();
    expect(start.value).toBe("0:10:00");
  });

  it("arrow keys nudge a handle, Shift nudges finely", async () => {
    const onRange = vi.fn();
    render(
      <TrimBar url={URL} duration={DUR} range={{ start: 600, end: 1200 }} onRange={onRange} />,
    );
    const handle = screen.getByLabelText("Startpunkt");
    // Long material → coarse step 5 s.
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(onRange).toHaveBeenCalledWith({ start: 605, end: 1200 });
    onRange.mockClear();
    fireEvent.keyDown(handle, { key: "ArrowLeft", shiftKey: true });
    expect(onRange).toHaveBeenCalledWith({ start: 599.9, end: 1200 });
  });
});
