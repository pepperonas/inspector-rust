export function relativeTime(unixMs: number): string {
  const diff = Date.now() - unixMs;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  if (diff < 604_800_000) return `${Math.floor(diff / 86_400_000)}d ago`;
  return new Date(unixMs).toLocaleDateString();
}

/** Full local date+time string for absolute timestamps in tooltips
 *  and click-to-reveal chips. Uses the user's locale via Intl —
 *  on macOS that's whatever System Settings → Language & Region says,
 *  matches Finder / mail / calendar formatting muscle memory. */
export function formatAbsolute(unixMs: number): string {
  return new Date(unixMs).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// Hard bound on how far the flattener scans. This runs per VISIBLE ROW per
/// keystroke, and a clip can be megabytes (pasted logs/JSON) — the old
/// whole-string `replace(/\s+/g, …)` allocated the full clip twice for an
/// 80-char label, which made typing visibly lag behind on log-heavy
/// histories. 64k covers any leading-whitespace run a real clip has before
/// its first `max` visible characters.
const TRUNCATE_SCAN_CAP = 65536;

const WS = /\s/;

export function truncateOneLine(text: string, max = 120): string {
  // Incremental scan: build the collapsed prefix and stop as soon as `max`
  // visible chars are known — O(max + skipped whitespace), never O(clip).
  let out = "";
  let lastWasSpace = true; // swallows leading whitespace
  const end = Math.min(text.length, TRUNCATE_SCAN_CAP);
  let i = 0;
  for (; i < end && out.length <= max; i++) {
    const c = text[i];
    if (WS.test(c)) {
      if (!lastWasSpace) {
        out += " ";
        lastWasSpace = true;
      }
    } else {
      out += c;
      lastWasSpace = false;
    }
  }
  const flat = out.trimEnd();
  // Ellipsis when the collapsed WHOLE string would exceed `max`: either the
  // prefix already overflowed, or we stopped early with real content still
  // ahead (bounded lookahead — a pathological multi-KB whitespace tail just
  // loses its ellipsis, nothing more).
  const more = flat.length > max || (i < text.length && /\S/.test(text.slice(i, i + 256)));
  if (!more) return flat;
  return flat.slice(0, Math.max(0, max - 1)) + "…";
}
