// The transcription ticker's model, pure so it can be tested without a DOM.
//
// The strip is a row of pieces: finals in arrival order, then at most one
// preview ("pending") piece per source at the end. The row moves left by
// translating ONE element; a piece never moves relative to the ones before
// it, so replacing a preview by its final text never shifts what is already
// on screen. When the newest text is fully visible the strip stops, so the
// last sentence stays readable until more arrives.

export type TickerSource = "mic" | "sys";

export interface Piece {
  id: string;
  source: TickerSource;
  text: string;
  final: boolean;
}

export const MAX_PIECES = 40;
/** At most this many times the base speed when text piles up off screen. */
export const MAX_SPEEDUP = 3;

function finals(pieces: Piece[]): Piece[] {
  return pieces.filter((p) => p.final);
}

function pendings(pieces: Piece[]): Piece[] {
  return pieces.filter((p) => !p.final);
}

function cap(pieces: Piece[]): Piece[] {
  if (pieces.length <= MAX_PIECES) return pieces;
  const pend = pendings(pieces);
  const fin = finals(pieces);
  return [...fin.slice(fin.length - (MAX_PIECES - pend.length)), ...pend];
}

/** A preview for `source`: one pending piece per source, kept at the end. */
export function applyHypothesis(
  pieces: Piece[],
  ev: { source: TickerSource; seq: number; stable: string; pending: string },
): Piece[] {
  const text = [ev.stable, ev.pending].filter((s) => s.trim() !== "").join(" ");
  const id = `p-${ev.source}`;
  if (text === "") return applyDropped(pieces, ev.source);
  const idx = pieces.findIndex((p) => p.id === id);
  if (idx >= 0) {
    const next = pieces.slice();
    next[idx] = { ...next[idx], text };
    return next;
  }
  return cap([...pieces, { id, source: ev.source, text, final: false }]);
}

/** A final: the source's preview goes, the final joins the finals. */
export function applyFinal(
  pieces: Piece[],
  ev: { source: TickerSource; seq: number; text: string },
): Piece[] {
  const rest = pieces.filter((p) => p.id !== `p-${ev.source}`);
  const piece: Piece = { id: `f-${ev.source}-${ev.seq}`, source: ev.source, text: ev.text, final: true };
  return cap([...finals(rest), piece, ...pendings(rest)]);
}

export function applyDropped(pieces: Piece[], source: TickerSource): Piece[] {
  return pieces.filter((p) => p.id !== `p-${source}`);
}

/**
 * How far to move left this frame. `end` is the strip's right edge in
 * viewport px, `anchor` where the newest text should come to rest.
 */
export function advance(end: number, anchor: number, dtMs: number, speedPxS: number, viewport: number): number {
  const backlog = end - anchor;
  if (backlog <= 0 || dtMs <= 0) return 0;
  const pile = Math.min(1, backlog / Math.max(1, viewport * 1.5));
  const mult = 1 + (MAX_SPEEDUP - 1) * pile;
  return Math.min(backlog, (speedPxS * mult * Math.min(dtMs, 100)) / 1000);
}

/**
 * Drop pieces that left the screen on the left and move the offset by their
 * widths, so nothing visible moves.
 */
export function prune(
  pieces: Piece[],
  widths: Record<string, number>,
  offset: number,
): { pieces: Piece[]; offset: number } {
  let i = 0;
  let off = offset;
  while (i < pieces.length - 1) {
    const w = widths[pieces[i].id];
    if (w === undefined || off + w >= 0) break;
    off += w;
    i += 1;
  }
  return { pieces: i === 0 ? pieces : pieces.slice(i), offset: off };
}

/** Show the MIC/SYS chip where the speaker changes. */
export function showsLabel(pieces: Piece[], index: number): boolean {
  return index === 0 || pieces[index - 1].source !== pieces[index].source;
}
