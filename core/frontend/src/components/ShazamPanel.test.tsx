import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";

// The status probe is held open so the test controls WHEN it resolves —
// the bug lived exactly in that gap.
let resolveBusy: (busy: boolean) => void = () => undefined;
const shazamListen = vi.fn(async () => null);
vi.mock("../lib/ipc", () => ({
  shazamListen: () => shazamListen(),
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
