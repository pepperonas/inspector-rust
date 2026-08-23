/**
 * Pure helpers for the `loc` panel (v0.117.0): language colours, the donut
 * geometry and number formatting. The component owns only SVG rendering.
 */

/**
 * GitHub-Linguist colours for the common languages (what people recognise
 * from repo language bars). Everything else gets a deterministic hash colour
 * — stable across runs, never the same tone twice in a row by accident.
 * Keys are tokei's `LanguageType::name()` strings.
 */
export const LANGUAGE_COLORS: Readonly<Record<string, string>> = {
  Rust: "#dea584",
  TypeScript: "#3178c6",
  TSX: "#3178c6",
  JavaScript: "#f1e05a",
  JSX: "#f1e05a",
  Python: "#3572A5",
  Go: "#00ADD8",
  Java: "#b07219",
  C: "#555555",
  "C++": "#f34b7d",
  "C#": "#178600",
  "C Header": "#555555",
  "C++ Header": "#f34b7d",
  HTML: "#e34c26",
  CSS: "#663399",
  Sass: "#c6538c",
  Shell: "#89e051",
  "Bourne Shell": "#89e051",
  Ruby: "#701516",
  PHP: "#4F5D95",
  Swift: "#F05138",
  Kotlin: "#A97BFF",
  Markdown: "#083fa1",
  JSON: "#8bc34a",
  YAML: "#cb171e",
  TOML: "#9c4221",
  SQL: "#e38c00",
  Dockerfile: "#384d54",
  Lua: "#000080",
  Haskell: "#5e5086",
  Elixir: "#6e4a7e",
  Dart: "#00B4AB",
  Vue: "#41b883",
  Svelte: "#ff3e00",
  Zig: "#ec915c",
  Perl: "#0298c3",
  R: "#198CE7",
  "Objective-C": "#438eff",
  TeX: "#3D6117",
  Assembly: "#6E4C13",
  "Plain Text": "#777777",
  XML: "#0060ac",
  Protobuf: "#4a90a4",
};

/** Deterministic fallback colour for languages outside the map: FNV-1a hash
 *  → a readable HSL tone (fixed s/l so it works on the dark theme). */
export function languageColor(name: string): string {
  const known = LANGUAGE_COLORS[name];
  if (known) return known;
  let h = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return `hsl(${h % 360}, 55%, 55%)`;
}

/** One donut segment, ready for an SVG `<path>`. */
export interface DonutSegment {
  name: string;
  color: string;
  /** 0..100 share. */
  pct: number;
  /** SVG path (`d`) of the ring segment. */
  d: string;
}

/** Point on a circle; angle in degrees, 0° = 12 o'clock, clockwise. */
function polar(cx: number, cy: number, r: number, angleDeg: number): [number, number] {
  const rad = ((angleDeg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
}

/**
 * Build donut ring segments from (name, pct) shares. Shares below `minPct`
 * are folded into an "Other" segment so the ring stays legible (a 0.2 %
 * sliver is a rendering artifact, not information). A full-circle single
 * segment is drawn as two half arcs — an SVG arc with identical start/end
 * points renders as NOTHING, the classic 100 % donut bug.
 */
export function donutSegments(
  shares: ReadonlyArray<{ name: string; pct: number }>,
  opts: { cx: number; cy: number; rOuter: number; rInner: number; minPct?: number },
): DonutSegment[] {
  const { cx, cy, rOuter, rInner } = opts;
  const minPct = opts.minPct ?? 2;
  const big = shares.filter((s) => s.pct >= minPct);
  const restPct = shares.filter((s) => s.pct < minPct).reduce((a, s) => a + s.pct, 0);
  const items = restPct > 0.05 ? [...big, { name: "Other", pct: restPct }] : [...big];
  const total = items.reduce((a, s) => a + s.pct, 0);
  if (total <= 0) return [];

  const segs: DonutSegment[] = [];
  let angle = 0;
  for (const item of items) {
    const sweep = (item.pct / total) * 360;
    const d =
      sweep >= 359.999
        ? // Full circle: two half-rings (see doc comment).
          fullRing(cx, cy, rOuter, rInner)
        : ringSegment(cx, cy, rOuter, rInner, angle, angle + sweep);
    segs.push({
      name: item.name,
      color: item.name === "Other" ? "#8b8b94" : languageColor(item.name),
      pct: item.pct,
      d,
    });
    angle += sweep;
  }
  return segs;
}

function ringSegment(
  cx: number,
  cy: number,
  rOuter: number,
  rInner: number,
  startDeg: number,
  endDeg: number,
): string {
  const largeArc = endDeg - startDeg > 180 ? 1 : 0;
  const [ox1, oy1] = polar(cx, cy, rOuter, startDeg);
  const [ox2, oy2] = polar(cx, cy, rOuter, endDeg);
  const [ix2, iy2] = polar(cx, cy, rInner, endDeg);
  const [ix1, iy1] = polar(cx, cy, rInner, startDeg);
  return (
    `M ${ox1.toFixed(3)} ${oy1.toFixed(3)} ` +
    `A ${rOuter} ${rOuter} 0 ${largeArc} 1 ${ox2.toFixed(3)} ${oy2.toFixed(3)} ` +
    `L ${ix2.toFixed(3)} ${iy2.toFixed(3)} ` +
    `A ${rInner} ${rInner} 0 ${largeArc} 0 ${ix1.toFixed(3)} ${iy1.toFixed(3)} Z`
  );
}

function fullRing(cx: number, cy: number, rOuter: number, rInner: number): string {
  return (
    ringSegment(cx, cy, rOuter, rInner, 0, 180) +
    " " +
    ringSegment(cx, cy, rOuter, rInner, 180, 360)
  );
}

/** 1234567 → "1.234.567" (de-style thousands separators — the panel's
 *  numbers are large and unbroken groups are unreadable). */
export function formatCount(n: number): string {
  return new Intl.NumberFormat("de-DE").format(n);
}

/** Percentage for the table: "42,3 %" style, one decimal, never "-0,0". */
export function formatPct(p: number): string {
  const v = Math.abs(p) < 0.05 ? 0 : p;
  return `${v.toFixed(1).replace(".", ",")} %`;
}
