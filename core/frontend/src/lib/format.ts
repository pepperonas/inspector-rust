import { locale, t } from "./i18n";

export function relativeTime(unixMs: number): string {
  const diff = Date.now() - unixMs;
  if (diff < 60_000) return t("time.just_now");
  if (diff < 3_600_000) return t("time.min_ago", { n: Math.floor(diff / 60_000) });
  if (diff < 86_400_000) return t("time.hours_ago", { n: Math.floor(diff / 3_600_000) });
  if (diff < 604_800_000) return t("time.days_ago", { n: Math.floor(diff / 86_400_000) });
  return new Date(unixMs).toLocaleDateString(locale());
}

/** Full local date+time string for absolute timestamps in tooltips and
 *  click-to-reveal chips, in the APP language. */
// One formatter per locale: `toLocaleString(undefined, options)` builds a
// fresh Intl.DateTimeFormat on EVERY call, and each visible history row
// formats its timestamp on render.
const ABSOLUTE_FMT = new Map<string, Intl.DateTimeFormat>();
export function formatAbsolute(unixMs: number): string {
  const loc = locale();
  let fmt = ABSOLUTE_FMT.get(loc);
  if (!fmt) {
    fmt = new Intl.DateTimeFormat(loc, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    ABSOLUTE_FMT.set(loc, fmt);
  }
  return fmt.format(new Date(unixMs));
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
