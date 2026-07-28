import type { ClipEntry } from "./types";

/**
 * The pinned-only history view (v0.94.0): given the already-search-filtered
 * clips (in the backend's `pinned DESC, last_used_at DESC` order), keep just
 * the pinned ones, order preserved. The query is applied upstream, so this is
 * the intersection of "matches the search" and "is pinned".
 *
 * Pure so the toolbar toggle's contract is unit-tested independently of the
 * React assembly in `App.tsx`.
 */
export function pinnedClips(clips: readonly ClipEntry[]): ClipEntry[] {
  return clips.filter((c) => c.pinned);
}
