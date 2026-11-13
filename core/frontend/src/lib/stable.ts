/**
 * Small helpers that keep object identities stable, so memoised components
 * and effects don't re-run for data that did not change.
 */
import type { ClipEntry, ListEntry } from "./types";

/** For a poll's `setState`: keep the previous object when the new reading is
 *  the same data — a fresh but equal object would re-render everything that
 *  receives it. */
export function keepIfEqual<T>(prev: T, next: T): T {
  if (Object.is(prev, next)) return prev;
  try {
    return JSON.stringify(prev) === JSON.stringify(next) ? prev : next;
  } catch {
    return next;
  }
}

const CLIP_ROWS = new WeakMap<ClipEntry, ListEntry>();

/** The list row for a clip — the same object for the same clip, so a row
 *  whose clip did not change keeps its memo across keystrokes. */
export function clipRow(clip: ClipEntry): ListEntry {
  let row = CLIP_ROWS.get(clip);
  if (!row) {
    row = { kind: "clip", data: clip };
    CLIP_ROWS.set(clip, row);
  }
  return row;
}

/** A tiny most-recently-used cache: `get` refreshes an entry, `set` drops the
 *  oldest one beyond `cap`. */
export class Lru<K, V> {
  private map = new Map<K, V>();
  constructor(private cap: number) {}
  get(key: K): V | undefined {
    const v = this.map.get(key);
    if (v === undefined) return undefined;
    this.map.delete(key);
    this.map.set(key, v);
    return v;
  }
  set(key: K, value: V): void {
    this.map.delete(key);
    this.map.set(key, value);
    while (this.map.size > this.cap) {
      const oldest = this.map.keys().next().value as K;
      this.map.delete(oldest);
    }
  }
  delete(key: K): void {
    this.map.delete(key);
  }
  get size(): number {
    return this.map.size;
  }
}
