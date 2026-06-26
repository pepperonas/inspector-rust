/**
 * Pure geometry for the Moom-style window palette's **hex grid** — the staggered
 * cell layout, pointer→cell hit-testing, and the cell-range → screen-fraction
 * mapping. No DOM; unit-tested. The React `WindowPalette` owns the SVG; Rust
 * turns the returned 0..1 fraction into an absolute window rect via the
 * Accessibility API.
 *
 * The grid is **visually** an offset hex tiling (odd rows shifted half a cell,
 * pointy-top look), but a *selection* is the rectangular bounding box of the
 * dragged-over cells, mapped to a rectangle of the target screen — exactly how
 * Moom behaves. The half-row offset is therefore cosmetic for the screen
 * mapping (which uses the regular col/row indices) but is honoured by the
 * nearest-center hit-test so the visible cell under the cursor is the one picked.
 */

export interface HexCell {
  col: number;
  row: number;
  /** Center x within the layout box. */
  cx: number;
  /** Center y within the layout box. */
  cy: number;
}

export interface FractionRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Centers of a `cols × rows` offset hex grid inside a `w × h` box. Odd rows are
 * shifted right by half a cell; the extra half-cell of width is reserved so the
 * shifted row still fits inside the box.
 */
export function hexCenters(cols: number, rows: number, w: number, h: number): HexCell[] {
  const out: HexCell[] = [];
  if (cols <= 0 || rows <= 0) return out;
  const cellW = w / (cols + 0.5);
  const cellH = h / rows;
  for (let row = 0; row < rows; row++) {
    const offset = (row % 2) * (cellW / 2);
    for (let col = 0; col < cols; col++) {
      out.push({ col, row, cx: offset + (col + 0.5) * cellW, cy: (row + 0.5) * cellH });
    }
  }
  return out;
}

/** The cell whose center is nearest `(px, py)`. Returns null for an empty grid. */
export function nearestCell(cells: HexCell[], px: number, py: number): HexCell | null {
  let best: HexCell | null = null;
  let bestD = Infinity;
  for (const c of cells) {
    const dx = c.cx - px;
    const dy = c.cy - py;
    const d = dx * dx + dy * dy;
    if (d < bestD) {
      bestD = d;
      best = c;
    }
  }
  return best;
}

/**
 * The 0..1 screen-fraction rectangle covered by the inclusive cell range between
 * `a` and `b` (order-independent), on a regular `cols × rows` grid. A single
 * cell → a `1/cols × 1/rows` tile; the whole grid → `{0,0,1,1}`.
 */
export function boundingFraction(
  a: { col: number; row: number },
  b: { col: number; row: number },
  cols: number,
  rows: number,
): FractionRect {
  const c0 = Math.min(a.col, b.col);
  const c1 = Math.max(a.col, b.col);
  const r0 = Math.min(a.row, b.row);
  const r1 = Math.max(a.row, b.row);
  return {
    x: c0 / cols,
    y: r0 / rows,
    w: (c1 - c0 + 1) / cols,
    h: (r1 - r0 + 1) / rows,
  };
}

/**
 * SVG `points` string for a pointy-top hexagon centered at `(cx, cy)` with
 * radii `rx`/`ry` (allows slightly squashed hexes to match the cell aspect).
 */
export function hexPolygon(cx: number, cy: number, rx: number, ry: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    // Pointy-top: first vertex straight up, then every 60°.
    const angle = (Math.PI / 180) * (60 * i - 90);
    const x = cx + rx * Math.cos(angle);
    const y = cy + ry * Math.sin(angle);
    pts.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  }
  return pts.join(" ");
}

/** Whether cell `(col,row)` is inside the inclusive bounding box of `a`..`b`. */
export function cellInRange(
  col: number,
  row: number,
  a: { col: number; row: number },
  b: { col: number; row: number },
): boolean {
  const c0 = Math.min(a.col, b.col);
  const c1 = Math.max(a.col, b.col);
  const r0 = Math.min(a.row, b.row);
  const r1 = Math.max(a.row, b.row);
  return col >= c0 && col <= c1 && row >= r0 && row <= r1;
}
