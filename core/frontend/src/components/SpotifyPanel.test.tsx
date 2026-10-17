import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SpotifyHistoryPage, SpotifyNow, SpotifyPlayRow } from "../lib/ipc";
import { parseSpotifyArg } from "../lib/spotify";

const ipc = vi.hoisted(() => ({
  now: null as unknown as SpotifyNow,
  page: { rows: [], total: 0, played_ms: 0 } as SpotifyHistoryPage,
  controls: [] as Array<[string, number | undefined]>,
  exports: [] as Array<[string, unknown]>,
  queries: [] as unknown[],
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../lib/ipc", () => ({
  SPOTIFY_NOT_INSTALLED: "spotify.not_installed",
  SPOTIFY_NOT_RUNNING: "spotify.not_running",
  SPOTIFY_DENIED: "spotify.automation_denied",
  getSpotifyConfig: vi.fn(async () => ({ history: true, artwork: false })),
  spotifyNow: vi.fn(async () => ipc.now),
  spotifyHistory: vi.fn(async (q: unknown) => {
    ipc.queries.push(q);
    return ipc.page;
  }),
  spotifyControl: vi.fn(async (a: string, v?: number) => {
    ipc.controls.push([a, v]);
  }),
  spotifyHistoryExport: vi.fn(async (f: string, q: unknown) => {
    ipc.exports.push([f, q]);
    return `/Users/x/Downloads/spotify-history.${f}`;
  }),
  spotifyOpenAutomationSettings: vi.fn(async () => {}),
}));

import { SpotifyPanel } from "./SpotifyPanel";
import de from "../locales/de.json";
import en from "../locales/en.json";
import { setCatalog } from "../lib/i18n";
import { useState } from "react";
import { useKeyboardNav } from "../hooks/useKeyboardNav";

// These pins were written against the German wording; run them in German.
beforeEach(() => setCatalog("de", de));
afterEach(() => setCatalog("en", en));

function now(over: Partial<SpotifyNow> = {}): SpotifyNow {
  return {
    installed: true,
    running: true,
    permission: "granted",
    state: "playing",
    track: {
      uri: "spotify:track:w",
      name: "Wicked Games",
      artist: "Parra for Cuva",
      album: "Paper Planes",
      duration_ms: 272_000,
      artwork_url: null,
    },
    position_ms: 30_000,
    volume: 69,
    at_ms: Date.now(),
    source: "applescript",
    played_ms: 30_000,
    history_enabled: true,
    artwork_enabled: false,
    error: null,
    ...over,
  };
}

function row(id: number, started: Date, over: Partial<SpotifyPlayRow> = {}): SpotifyPlayRow {
  return {
    id,
    started_at: started.getTime(),
    ended_at: started.getTime() + 261_000,
    track_uri: "",
    track: `Track ${id}`,
    artist: "Artist",
    album: "Album",
    duration_ms: 272_000,
    played_ms: 261_000,
    status: "listened",
    artwork_url: null,
    open: false,
    ...over,
  };
}

afterEach(() => {
  cleanup();
  ipc.controls = [];
  ipc.exports = [];
  ipc.queries = [];
});

const noop = () => {};

describe("SpotifyPanel", () => {
  it("shows the current track and controls it with the keyboard once focused", async () => {
    ipc.now = now();
    ipc.page = { rows: [], total: 0, played_ms: 0 };
    render(<SpotifyPanel cmd={parseSpotifyArg("")} focused runSignal={0} onExit={noop} onQuery={noop} />);
    await screen.findByText("Wicked Games");
    expect(screen.getByText("Parra for Cuva")).toBeTruthy();
    fireEvent.keyDown(window, { key: " " });
    fireEvent.keyDown(window, { key: "ArrowRight" });
    await waitFor(() => expect(ipc.controls).toEqual([["toggle", undefined], ["next", undefined]]));
  });

  it("explains when Spotify is not running", async () => {
    ipc.now = now({ running: false, state: "not_running", track: null });
    render(<SpotifyPanel cmd={parseSpotifyArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await screen.findByText(/Spotify läuft nicht/);
  });

  it("groups the history by day and shows the totals", async () => {
    const today = new Date();
    today.setHours(2, 24, 0, 0);
    const yesterday = new Date(today.getTime() - 86_400_000);
    ipc.page = {
      rows: [row(1, today), row(2, yesterday, { status: "skipped", played_ms: 18_000 })],
      total: 2,
      played_ms: 279_000,
    };
    render(<SpotifyPanel cmd={parseSpotifyArg("history")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await screen.findByText("Heute");
    expect(screen.getByText("Gestern")).toBeTruthy();
    expect(screen.getAllByTestId("spotify-row")).toHaveLength(2);
    expect(screen.getByText("übersprungen")).toBeTruthy();
    expect(screen.getByTestId("spotify-summary").textContent).toBe("2 Einträge · 5 Min. gehört");
  });

  it("passes the filter to the query and exports with it", async () => {
    ipc.page = { rows: [row(1, new Date())], total: 1, played_ms: 261_000 };
    const cmd = parseSpotifyArg("history artist Kanye West");
    render(<SpotifyPanel cmd={cmd} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await screen.findByTestId("spotify-row");
    expect(ipc.queries[0]).toMatchObject({ artist: "Kanye West", limit: 100, offset: 0 });
    fireEvent.click(screen.getByText("csv"));
    await screen.findByText(/Exportiert: spotify-history.csv/);
    expect(ipc.exports[0]).toEqual(["csv", expect.objectContaining({ artist: "Kanye West" })]);
  });

  it("runs an Enter action exactly once per signal, never on remount", async () => {
    ipc.now = now();
    const cmd = parseSpotifyArg("volume 40");
    const { rerender, unmount } = render(
      <SpotifyPanel cmd={cmd} focused={false} runSignal={0} onExit={noop} onQuery={noop} />,
    );
    expect(ipc.controls).toEqual([]);
    rerender(<SpotifyPanel cmd={cmd} focused runSignal={1} onExit={noop} onQuery={noop} />);
    await waitFor(() => expect(ipc.controls).toEqual([["volume", 40]]));
    rerender(<SpotifyPanel cmd={cmd} focused runSignal={1} onExit={noop} onQuery={noop} />);
    unmount();
    render(<SpotifyPanel cmd={cmd} focused runSignal={1} onExit={noop} onQuery={noop} />);
    await act(async () => {});
    expect(ipc.controls).toEqual([["volume", 40]]);
  });

  it("Esc leaves the panel", async () => {
    ipc.now = now();
    const onExit = vi.fn();
    render(<SpotifyPanel cmd={parseSpotifyArg("")} focused runSignal={0} onExit={onExit} onQuery={noop} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).toHaveBeenCalled();
  });

  it("runs every control and export through Enter repeatedly while the panel is focused", async () => {
    function Commands() {
      const [arg, setArg] = useState("");
      const [focused, setFocused] = useState(false);
      const [signal, setSignal] = useState(100);
      useKeyboardNav({
        length: 1, selected: 0, setSelected: noop,
        onEnter: () => { setFocused(true); setSignal((n) => n + 1); },
        onEscape: noop,
      });
      return <>
        <input aria-label="Spotify command" value={arg} onChange={(e) => setArg(e.target.value)} />
        <SpotifyPanel cmd={parseSpotifyArg(arg)} focused={focused} runSignal={signal} onExit={noop} onQuery={noop} />
      </>;
    }
    ipc.now = now();
    render(<Commands />);
    const input = screen.getByLabelText("Spotify command");
    const controls: Array<[string, string, number?]> = [
      ["play", "play"], ["pause", "pause"], ["stop", "pause"], ["pause", "pause"],
      ["toggle", "toggle"], ["next", "next"], ["weiter", "next"], ["skip", "next"],
      ["previous", "previous"], ["prev", "previous"], ["back", "previous"], ["zurück", "previous"],
      ["volume 7", "volume", 7], ["vol 0", "volume", 0], ["lautstärke 100", "volume", 100],
      ["seek 90", "seek", 90], ["seek 1:30", "seek", 90], ["spulen 1:02:05", "seek", 3725],
    ];
    for (const [arg, action, value] of controls) {
      const count = ipc.controls.length;
      fireEvent.change(input, { target: { value: arg } });
      expect(ipc.controls).toHaveLength(count); // typing never runs it
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(ipc.controls).toHaveLength(count + 1));
      expect(ipc.controls[ipc.controls.length - 1]).toEqual([action, value]);
    }
    for (const format of ["json", "csv", "md"]) {
      const count = ipc.exports.length;
      fireEvent.change(input, { target: { value: `history today export ${format}` } });
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(ipc.exports).toHaveLength(count + 1));
      expect(ipc.exports[ipc.exports.length - 1]).toEqual([format, expect.objectContaining({ since_ms: expect.any(Number) })]);
    }
  });
});
