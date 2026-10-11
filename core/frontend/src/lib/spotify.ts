/**
 * `spotify` — pure helpers: argument grammar, date ranges, day grouping and
 * formatting for the Spotify panel. The history itself is recorded in Rust
 * (`spotify/`); see docs/spotify.md.
 */
import type { SpotifyControl, SpotifyHistoryQuery, SpotifyPlayRow } from "./ipc";

export type HistoryRange = "today" | "yesterday" | "week" | "month";
export type ExportFormat = "json" | "csv" | "md";

export interface HistoryFilter {
  range?: HistoryRange;
  artist?: string;
  text?: string;
}

export type SpotifyCmd =
  | { kind: "now" }
  | { kind: "history"; filter: HistoryFilter; export?: ExportFormat }
  | { kind: "control"; action: SpotifyControl; value?: number }
  | { kind: "invalid"; message: string };

const HISTORY_WORDS = new Set(["history", "verlauf", "h"]);
const RANGES: Record<string, HistoryRange> = {
  today: "today",
  heute: "today",
  yesterday: "yesterday",
  gestern: "yesterday",
  week: "week",
  woche: "week",
  month: "month",
  monat: "month",
};
const ARTIST_WORDS = new Set(["artist", "künstler", "kuenstler"]);
const CONTROLS: Record<string, SpotifyControl> = {
  play: "play",
  pause: "pause",
  toggle: "toggle",
  next: "next",
  weiter: "next",
  skip: "next",
  previous: "previous",
  prev: "previous",
  back: "previous",
  zurück: "previous",
};

/** Every first word the grammar knows (for "still typing it"). */
const KNOWN_HEADS = [
  "now", "jetzt", ...HISTORY_WORDS, ...Object.keys(CONTROLS), "volume", "vol", "lautstärke", "seek", "spulen",
];

/** `90`, `1:30` or `1:02:05` → seconds; null if not a time. */
export function parseSeconds(s: string): number | null {
  const t = s.trim();
  if (/^\d+(?:[.,]\d+)?$/.test(t)) return Number(t.replace(",", "."));
  const m = /^(\d+):([0-5]\d)(?::([0-5]\d))?$/.exec(t);
  if (!m) return null;
  return m[3] != null
    ? Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3])
    : Number(m[1]) * 60 + Number(m[2]);
}

function parseHistory(words: string[]): SpotifyCmd {
  const filter: HistoryFilter = {};
  let exportFormat: ExportFormat | undefined;
  const text: string[] = [];
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    const lower = w.toLowerCase();
    if (lower === "export") {
      const next = (words[i + 1] ?? "").toLowerCase();
      if (next === "json" || next === "csv" || next === "md" || next === "markdown") {
        exportFormat = next === "markdown" ? "md" : next;
        i++;
      } else {
        exportFormat = "json";
      }
    } else if (ARTIST_WORDS.has(lower) && i + 1 >= words.length) {
      // `artist` with the name still to come: no filter yet, not a search.
    } else if (RANGES[lower] && !filter.range) {
      filter.range = RANGES[lower];
    } else if (ARTIST_WORDS.has(lower) && i + 1 < words.length && filter.artist == null) {
      // Everything up to an `export` keyword is the artist's name.
      const rest: string[] = [];
      while (i + 1 < words.length && words[i + 1].toLowerCase() !== "export") rest.push(words[++i]);
      filter.artist = rest.join(" ");
    } else {
      text.push(w);
    }
  }
  if (text.length) filter.text = text.join(" ");
  return { kind: "history", filter, export: exportFormat };
}

/** The `spotify …` argument. Bare = now playing. */
export function parseSpotifyArg(arg: string): SpotifyCmd {
  const words = arg.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return { kind: "now" };
  const head = words[0].toLowerCase();
  if (head === "now" || head === "jetzt") return { kind: "now" };
  if (HISTORY_WORDS.has(head)) return parseHistory(words.slice(1));
  if (CONTROLS[head]) return { kind: "control", action: CONTROLS[head] };
  if (head === "volume" || head === "vol" || head === "lautstärke") {
    const v = words[1] != null ? Number(words[1].replace(",", ".")) : NaN;
    if (!Number.isFinite(v) || v < 0 || v > 100) {
      return { kind: "invalid", message: "Lautstärke 0–100 angeben, z. B. spotify volume 40" };
    }
    return { kind: "control", action: "volume", value: Math.round(v) };
  }
  if (head === "seek" || head === "spulen") {
    const s = words[1] != null ? parseSeconds(words[1]) : null;
    if (s == null) return { kind: "invalid", message: "Position angeben, z. B. spotify seek 90 oder 1:30" };
    return { kind: "control", action: "seek", value: s };
  }
  // The start of a known word is not an error yet — the user is still
  // typing it (Tab completes it).
  if (words.length === 1 && KNOWN_HEADS.some((k) => k.startsWith(head))) return { kind: "now" };
  return {
    kind: "invalid",
    message: `Unbekannt: „${words[0]}“ — now, history, play, pause, toggle, next, previous, volume, seek`,
  };
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Local-time bounds of a range (DST-safe: built from calendar dates). */
export function rangeBounds(range: HistoryRange, now: Date): { since_ms: number; until_ms?: number } {
  const today = startOfDay(now);
  const daysBack = (n: number) =>
    new Date(now.getFullYear(), now.getMonth(), now.getDate() - n).getTime();
  switch (range) {
    case "today":
      return { since_ms: today };
    case "yesterday":
      return { since_ms: daysBack(1), until_ms: today };
    case "week":
      return { since_ms: daysBack(6) };
    case "month":
      return { since_ms: daysBack(29) };
  }
}

export function toQuery(filter: HistoryFilter, now: Date, limit?: number, offset?: number): SpotifyHistoryQuery {
  return {
    text: filter.text ?? null,
    artist: filter.artist ?? null,
    ...(filter.range ? rangeBounds(filter.range, now) : {}),
    limit: limit ?? null,
    offset: offset ?? null,
  };
}

export interface DayGroup {
  key: string;
  label: string;
  rows: SpotifyPlayRow[];
}

/** `Heute` / `Gestern` / `Mi., 8. Okt.` (+ year when not this year). */
export function dayLabel(ms: number, now: Date): string {
  const day = startOfDay(new Date(ms));
  const today = startOfDay(now);
  if (day === today) return "Heute";
  if (day === new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1).getTime()) return "Gestern";
  const d = new Date(ms);
  return d.toLocaleDateString("de-DE", {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(d.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {}),
  });
}

/** Rows (newest first) → consecutive day groups. */
export function groupByDay(rows: SpotifyPlayRow[], now: Date): DayGroup[] {
  const out: DayGroup[] = [];
  for (const r of rows) {
    const d = new Date(r.started_at);
    const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    const last = out[out.length - 1];
    if (last && last.key === key) last.rows.push(r);
    else out.push({ key, label: dayLabel(r.started_at, now), rows: [r] });
  }
  return out;
}

/** `4:21`, `1:02:05`. */
export function formatClock(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** `12 Min.` / `3 Std. 5 Min.` for totals. */
export function formatTotal(ms: number): string {
  const min = Math.round(ms / 60000);
  if (min < 60) return `${min} Min.`;
  const h = Math.floor(min / 60);
  return min % 60 ? `${h} Std. ${min % 60} Min.` : `${h} Std.`;
}

export function timeOfDay(ms: number): string {
  return new Date(ms).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
}

export const STATUS_LABEL: Record<SpotifyPlayRow["status"], string> = {
  listened: "gehört",
  partial: "angespielt",
  skipped: "übersprungen",
  playing: "läuft",
  paused: "pausiert",
};

/** How much of a play was heard, 0..1 — `null` when the length is unknown.
 *  Drives the progress ring of a "partial" icon; a missing length must not
 *  be drawn as a guess. */
export function playedFraction(row: { played_ms: number; duration_ms: number | null }): number | null {
  if (row.duration_ms == null || !(row.duration_ms > 0)) return null;
  const f = row.played_ms / row.duration_ms;
  if (!Number.isFinite(f)) return null;
  return Math.min(1, Math.max(0, f));
}

/** One-line label for the command row. */
export function commandLabel(cmd: SpotifyCmd): string {
  switch (cmd.kind) {
    case "now":
      return "Spotify — läuft gerade";
    case "invalid":
      return `spotify: ${cmd.message}`;
    case "control": {
      const what: Record<SpotifyControl, string> = {
        play: "abspielen",
        pause: "pausieren",
        toggle: "Wiedergabe umschalten",
        next: "nächster Titel",
        previous: "vorheriger Titel",
        volume: `Lautstärke ${cmd.value}`,
        seek: `springen zu ${formatClock((cmd.value ?? 0) * 1000)}`,
      };
      return `Spotify: ${what[cmd.action]}`;
    }
    case "history": {
      const parts: string[] = [];
      if (cmd.filter.range) {
        parts.push({ today: "heute", yesterday: "gestern", week: "7 Tage", month: "30 Tage" }[cmd.filter.range]);
      }
      if (cmd.filter.artist) parts.push(`von ${cmd.filter.artist}`);
      if (cmd.filter.text) parts.push(`„${cmd.filter.text}“`);
      const base = cmd.export ? `Spotify-Verlauf als ${cmd.export.toUpperCase()} exportieren` : "Spotify-Verlauf";
      return parts.length ? `${base} · ${parts.join(" · ")}` : base;
    }
  }
}

/** Position now, extrapolated from a reading while playing. */
export function livePosition(positionMs: number | null, atMs: number, playing: boolean, nowMs: number, durationMs: number | null): number | null {
  if (positionMs == null) return null;
  const pos = playing ? positionMs + Math.max(0, nowMs - atMs) : positionMs;
  return durationMs != null ? Math.min(pos, durationMs) : pos;
}
