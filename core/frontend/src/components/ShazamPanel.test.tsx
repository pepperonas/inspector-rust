import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";

// The status probe is held open so the test controls WHEN it resolves —
// the bug lived exactly in that gap.
let resolveBusy: (busy: boolean) => void = () => undefined;
const shazamListen = vi.fn(async () => null);
const lyricsCapture = vi.fn(async () => ({
  matched: { title: "Testlied", artist: "Testband", cover_url: "", shazam_url: "", spotify_url: "", youtube_url: "" },
  lyrics: "erste Zeile\nzweite Zeile",
  copied: true,
  error: null,
}));
vi.mock("../lib/ipc", () => ({
  shazamListen: () => shazamListen(),
  lyricsCapture: () => lyricsCapture(),
  shazamIsListening: () =>
    new Promise<boolean>((r) => {
      resolveBusy = r;
    }),
  openSpotify: async () => undefined,
  shazamHistoryList: async () => [],
  shazamHistoryClear: async () => undefined,
  shazamHistoryDelete: async () => undefined,
  shazamLyrics: async () => null,
  shazamLyricsTranslated: async () => null,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));

import { ShazamPanel } from "./ShazamPanel";

afterEach(() => {
  cleanup();
  shazamListen.mockClear();
  lyricsCapture.mockClear();
});

describe("ShazamPanel mount race", () => {
  it("never opens the mic when the panel closed while the status probe was in flight", async () => {
    const { unmount } = render(<ShazamPanel focused={false} onExit={() => undefined} />);
    unmount();
    resolveBusy(false);
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(shazamListen).not.toHaveBeenCalled();
  });

  it("still starts listening when the panel stays open", async () => {
    render(<ShazamPanel focused={false} onExit={() => undefined} />);
    resolveBusy(false);
    await vi.waitFor(() => expect(shazamListen).toHaveBeenCalledTimes(1));
  });
});

describe("ShazamPanel lyrics capture", () => {
  it("captures lyrics instead of a plain recognition and says they were copied", async () => {
    const view = render(<ShazamPanel focused={false} captureLyrics onExit={() => undefined} />);
    resolveBusy(false);
    await vi.waitFor(() => expect(view.getByTestId("lyrics-auto-copied")).toBeTruthy());
    expect(lyricsCapture).toHaveBeenCalledTimes(1);
    expect(shazamListen).not.toHaveBeenCalled();
    expect(view.getByText(/erste Zeile/)).toBeTruthy();
    expect(view.getByText("Testlied")).toBeTruthy();
  });
});
