import { describe, expect, it } from "vitest";
import type { SpotifyPlayRow } from "./ipc";
import {
  commandLabel,
  dayLabel,
  formatClock,
  formatTotal,
  groupByDay,
  livePosition,
  parseSeconds,
  parseSpotifyArg,
  rangeBounds,
  toQuery,
} from "./spotify";

function row(id: number, started: Date): SpotifyPlayRow {
  return {
    id,
    started_at: started.getTime(),
    ended_at: started.getTime() + 60_000,
    track_uri: `spotify:track:${id}`,
    track: `T${id}`,
    artist: "A",
    album: "B",
    duration_ms: 200_000,
    played_ms: 60_000,
    status: "partial",
    artwork_url: null,
    open: false,
  };
}

describe("parseSpotifyArg", () => {
  it("bare and now show what is playing", () => {
    expect(parseSpotifyArg("")).toEqual({ kind: "now" });
    expect(parseSpotifyArg("  now ")).toEqual({ kind: "now" });
  });

  it("history with ranges, artist, search and export", () => {
    expect(parseSpotifyArg("history")).toEqual({ kind: "history", filter: {}, export: undefined });
    expect(parseSpotifyArg("history today")).toEqual({ kind: "history", filter: { range: "today" }, export: undefined });
    expect(parseSpotifyArg("verlauf woche")).toEqual({ kind: "history", filter: { range: "week" }, export: undefined });
    expect(parseSpotifyArg("history artist Parra for Cuva")).toEqual({
      kind: "history",
      filter: { artist: "Parra for Cuva" },
      export: undefined,
    });
    expect(parseSpotifyArg("history wicked games")).toEqual({
      kind: "history",
      filter: { text: "wicked games" },
      export: undefined,
    });
    expect(parseSpotifyArg("history export")).toEqual({ kind: "history", filter: {}, export: "json" });
    expect(parseSpotifyArg("history export csv")).toEqual({ kind: "history", filter: {}, export: "csv" });
    expect(parseSpotifyArg("history week artist Kanye West export markdown")).toEqual({
      kind: "history",
      filter: { range: "week", artist: "Kanye West" },
      export: "md",
    });
  });

  it("controls with validated values", () => {
    expect(parseSpotifyArg("next")).toEqual({ kind: "control", action: "next" });
    expect(parseSpotifyArg("prev")).toEqual({ kind: "control", action: "previous" });
    expect(parseSpotifyArg("toggle")).toEqual({ kind: "control", action: "toggle" });
    expect(parseSpotifyArg("volume 40")).toEqual({ kind: "control", action: "volume", value: 40 });
    expect(parseSpotifyArg("volume 140").kind).toBe("invalid");
    expect(parseSpotifyArg("volume").kind).toBe("invalid");
    expect(parseSpotifyArg("seek 1:30")).toEqual({ kind: "control", action: "seek", value: 90 });
    expect(parseSpotifyArg("seek x").kind).toBe("invalid");
    expect(parseSpotifyArg("dance").kind).toBe("invalid");
  });
});

describe("helpers", () => {
  it("parses times", () => {
    expect(parseSeconds("90")).toBe(90);
    expect(parseSeconds("1:30")).toBe(90);
    expect(parseSeconds("1:02:05")).toBe(3725);
    expect(parseSeconds("1:75")).toBeNull();
  });

  it("builds local-day ranges", () => {
    const now = new Date(2026, 9, 11, 2, 24);
    expect(rangeBounds("today", now)).toEqual({ since_ms: new Date(2026, 9, 11).getTime() });
    expect(rangeBounds("yesterday", now)).toEqual({
      since_ms: new Date(2026, 9, 10).getTime(),
      until_ms: new Date(2026, 9, 11).getTime(),
    });
    expect(rangeBounds("week", now).since_ms).toBe(new Date(2026, 9, 5).getTime());
    expect(toQuery({ text: "x" }, now, 50, 100)).toEqual({ text: "x", artist: null, limit: 50, offset: 100 });
  });

  it("groups rows by local day with readable labels", () => {
    const now = new Date(2026, 9, 11, 2, 30);
    const groups = groupByDay(
      [
        row(1, new Date(2026, 9, 11, 2, 24)),
        row(2, new Date(2026, 9, 11, 0, 5)),
        row(3, new Date(2026, 9, 10, 23, 51)),
        row(4, new Date(2026, 9, 8, 12, 0)),
      ],
      now,
    );
    expect(groups.map((g) => [g.label, g.rows.length])).toEqual([
      ["Heute", 2],
      ["Gestern", 1],
      [dayLabel(new Date(2026, 9, 8).getTime(), now), 1],
    ]);
    expect(dayLabel(new Date(2025, 0, 2).getTime(), now)).toContain("2025");
  });

  it("formats durations", () => {
    expect(formatClock(261_000)).toBe("4:21");
    expect(formatClock(3_725_000)).toBe("1:02:05");
    expect(formatTotal(12 * 60000)).toBe("12 Min.");
    expect(formatTotal(185 * 60000)).toBe("3 Std. 5 Min.");
  });

  it("extrapolates the position only while playing and stops at the end", () => {
    expect(livePosition(10_000, 0, true, 5_000, 200_000)).toBe(15_000);
    expect(livePosition(10_000, 0, false, 5_000, 200_000)).toBe(10_000);
    expect(livePosition(199_000, 0, true, 5_000, 200_000)).toBe(200_000);
    expect(livePosition(null, 0, true, 5_000, null)).toBeNull();
  });

  it("labels the command row", () => {
    expect(commandLabel(parseSpotifyArg("next"))).toBe("Spotify: nächster Titel");
    expect(commandLabel(parseSpotifyArg("history today export csv"))).toBe(
      "Spotify-Verlauf als CSV exportieren · heute",
    );
  });
});
