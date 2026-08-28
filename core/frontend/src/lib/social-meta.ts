import type { SocialMeta } from "./ipc";

/** `213` → `3:33`; `3661` → `1:01:01`. Empty for unknown. */
export function formatDuration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !Number.isFinite(secs) || secs < 0) return "";
  const s = Math.floor(secs);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${m}:${pad(sec)}`;
}

/** First `max` characters on a word boundary, with an ellipsis when cut. */
export function clampDescription(text: string, max: number): string {
  const t = text.replace(/\s+/g, " ").trim();
  if (t.length <= max) return t;
  const cut = t.slice(0, max);
  const sp = cut.lastIndexOf(" ");
  return (sp > max * 0.6 ? cut.slice(0, sp) : cut).trimEnd() + "…";
}

export type MetaState =
  | { state: "loading" }
  | { state: "ok"; meta: SocialMeta }
  | { state: "failed"; error: string };

/**
 * Loads metadata for many links without hammering yt-dlp.
 *
 * ⚠️ Each call costs ~4 s (measured), so three things are not optional:
 * a **cache** per URL (the grabber re-renders on every keystroke), **in-flight
 * deduplication** (the same URL must never be fetched twice at once), and a
 * **concurrency cap** — a pasted list of thirty links would otherwise spawn
 * thirty yt-dlp processes at the same moment.
 *
 * Injecting the fetcher keeps all of that testable without the backend.
 */
export function createMetaLoader(
  fetchMeta: (url: string) => Promise<SocialMeta>,
  cap = 3,
) {
  const cache = new Map<string, MetaState>();
  const queue: string[] = [];
  let running = 0;
  const listeners = new Set<() => void>();
  const notify = () => listeners.forEach((l) => l());

  const pump = () => {
    while (running < cap && queue.length > 0) {
      const url = queue.shift()!;
      running += 1;
      fetchMeta(url)
        .then((meta) => cache.set(url, { state: "ok", meta }))
        .catch((e) => cache.set(url, { state: "failed", error: String(e) }))
        .finally(() => {
          running -= 1;
          notify();
          pump();
        });
    }
  };

  return {
    get(url: string): MetaState | undefined {
      return cache.get(url);
    },
    /** Queue every unknown URL; already cached or in-flight ones are skipped. */
    request(urls: readonly string[]) {
      let added = false;
      for (const url of urls) {
        if (cache.has(url)) continue;
        cache.set(url, { state: "loading" });
        queue.push(url);
        added = true;
      }
      if (added) {
        notify();
        pump();
      }
    },
    subscribe(fn: () => void) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    /** Test/diagnostic view: how many fetches are in flight right now. */
    inFlight() {
      return running;
    },
  };
}
