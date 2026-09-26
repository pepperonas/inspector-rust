import type { Snippet } from "./types";

/**
 * Whether two snippet-lookup results are the same for rendering purposes.
 * Lets the search effect keep the previous array (and so the memoised
 * `combined` list) when a keystroke's lookup returns what is already shown.
 * An edit changes `updated_at`/`body`, a regroup changes `category` — either
 * must count as a change, or the preview would keep the stale snippet.
 */
export function sameSnippetList(a: Snippet[], b: Snippet[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  return a.every((x, i) => {
    const y = b[i];
    return (
      x.id === y.id &&
      x.updated_at === y.updated_at &&
      x.body === y.body &&
      x.title === y.title &&
      x.abbreviation === y.abbreviation &&
      (x.category ?? null) === (y.category ?? null) &&
      (x.version ?? 1) === (y.version ?? 1)
    );
  });
}
