// Pure helpers for the `meeting` panel and its settings.

import { locale, t, type MessageKey } from "./i18n";
import type { MeetingSession, MeetingSource } from "./ipc";

/** `12:34` or `1:02:03` since the session started. */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(sec)}` : `${m}:${two(sec)}`;
}

export function sourceLabel(s: MeetingSource): string {
  return s === "mic" ? "MIC" : "SYS";
}

/** `HH:MM:SS` local time of a line. */
export function clockTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** "3. Nov. 2026, 19:45" for a session row. */
export function sessionTitle(meta: MeetingSession["meta"]): string {
  return new Date(meta.started_ms).toLocaleString(locale(), {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** "574 MB" — decimal units, like Finder. */
export function modelSizeLabel(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1).replace(".", locale().startsWith("de") ? "," : ".")} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

/** Download progress 0..100, or null when the size is unknown. */
export function downloadPercent(done: number, total: number): number | null {
  if (!(total > 0)) return null;
  return Math.min(100, Math.max(0, Math.round((done / total) * 100)));
}

/** A backend error sentinel as a sentence (falls back to the raw text). */
export function errorText(e: string | null | undefined): string | null {
  if (!e) return null;
  const key = `meeting.err.${e.replace(/\./g, "_")}` as MessageKey;
  const s = t(key);
  return s === key ? e : s;
}

/** Level 0..1 from an RMS value (−60 dBFS … 0 dBFS). */
export function levelOf(rms: number): number {
  if (!(rms > 0)) return 0;
  const db = 20 * Math.log10(rms);
  return Math.min(1, Math.max(0, (db + 60) / 60));
}
