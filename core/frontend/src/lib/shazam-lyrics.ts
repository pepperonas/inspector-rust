/**
 * Pure helpers for the Shazam lyrics view (clipboard formatting).
 */

export type LyricSeg = { orig: string; trans: string };

/** Format bilingual segments for the clipboard: each orig line, then its
 *  German translation on the next line, blank line between pairs. When the
 *  source is already German (`srcLang === "de"`), only the original lines. */
export function formatBilingualForCopy(
  segments: readonly LyricSeg[],
  srcLang: string,
): string {
  if (srcLang === "de") {
    return segments.map((s) => s.orig).join("\n");
  }
  return segments
    .map((s) => {
      const o = s.orig.trimEnd();
      const t = s.trans.trimEnd();
      if (!t || t === o) return o;
      return `${o}\n${t}`;
    })
    .join("\n\n");
}
