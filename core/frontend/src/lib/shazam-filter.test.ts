import { describe, expect, it } from "vitest";
import { filterShazamHistory } from "./shazam-filter";
import type { ShazamHistoryEntry } from "./ipc";

const entry = (over: Partial<ShazamHistoryEntry>): ShazamHistoryEntry => ({
  id: 1,
  recognized_at: 0,
  title: "",
  artist: "",
  cover_url: "",
  shazam_url: "",
  spotify_url: "",
  youtube_url: "",
  genre: "",
  album: "",
  released: "",
  ...over,
});

const LIST: ShazamHistoryEntry[] = [
  entry({ id: 1, title: "Levitating", artist: "Dua Lipa", album: "Future Nostalgia", genre: "Pop" }),
  entry({ id: 2, title: "Bohemian Rhapsody", artist: "Queen", album: "A Night at the Opera", genre: "Rock" }),
  entry({ id: 3, title: "99 Luftballons", artist: "Nena", album: "Nena", genre: "NDW" }),
];

describe("filterShazamHistory", () => {
  it("empty / whitespace query returns everything (copy, not the same ref)", () => {
    expect(filterShazamHistory(LIST, "")).toEqual(LIST);
    expect(filterShazamHistory(LIST, "   ")).toEqual(LIST);
    expect(filterShazamHistory(LIST, "")).not.toBe(LIST);
  });

  it("matches title, artist, album and genre — case-insensitively", () => {
    expect(filterShazamHistory(LIST, "levit").map((e) => e.id)).toEqual([1]);
    expect(filterShazamHistory(LIST, "QUEEN").map((e) => e.id)).toEqual([2]);
    expect(filterShazamHistory(LIST, "opera").map((e) => e.id)).toEqual([2]);
    expect(filterShazamHistory(LIST, "ndw").map((e) => e.id)).toEqual([3]);
  });

  it("multiple terms must ALL match, across different fields", () => {
    expect(filterShazamHistory(LIST, "dua levit").map((e) => e.id)).toEqual([1]);
    expect(filterShazamHistory(LIST, "dua queen")).toEqual([]);
  });

  it("digits and no-match queries behave", () => {
    expect(filterShazamHistory(LIST, "99").map((e) => e.id)).toEqual([3]);
    expect(filterShazamHistory(LIST, "zzz")).toEqual([]);
  });

  it("preserves the input order", () => {
    expect(filterShazamHistory(LIST, "a").map((e) => e.id)).toEqual([1, 2, 3]);
  });
});
