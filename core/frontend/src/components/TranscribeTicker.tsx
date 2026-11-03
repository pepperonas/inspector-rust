import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getMeetingConfig, type TickerConfig } from "../lib/ipc";
import {
  advance,
  applyDropped,
  applyFinal,
  applyHypothesis,
  prune,
  showsLabel,
  type Piece,
  type TickerSource,
} from "../lib/ticker";

/** Where the newest text comes to rest: this far from the right edge. */
const REST_GAP = 24;

const ACCENT: Record<TickerSource, string> = { mic: "#38bdf8", sys: "#f59e0b" };

const DEFAULTS: TickerConfig = {
  visible: true,
  position: "bottom",
  display: null,
  width_pct: 70,
  font_px: 15,
  opacity: 0.85,
  speed_px_s: 60,
};

/**
 * The transcription ticker (its own transparent, click-through window).
 * Finals in light text, previews grey and italic; a preview is replaced by
 * its final in place. One element moves, by transform only, in one rAF loop.
 */
export function TranscribeTicker() {
  const [cfg, setCfg] = useState<TickerConfig>(DEFAULTS);
  const [pieces, setPieces] = useState<Piece[]>([]);
  const stripRef = useRef<HTMLDivElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const widths = useRef<Record<string, number>>({});
  const offset = useRef<number | null>(null);
  const piecesRef = useRef<Piece[]>([]);
  const speedRef = useRef(DEFAULTS.speed_px_s);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    void getMeetingConfig()
      .then((c) => setCfg(c.ticker))
      .catch(() => {});
    const offs = [
      listen<TickerConfig>("meeting-ticker-config", (e) => setCfg(e.payload)),
      listen<{ source: TickerSource; seq: number; stable: string; pending: string }>("meeting-hypothesis", (e) =>
        setPieces((p) => applyHypothesis(p, e.payload)),
      ),
      listen<{ source: TickerSource; seq: number; text: string }>("meeting-line", (e) =>
        setPieces((p) => applyFinal(p, e.payload)),
      ),
      listen<{ source: TickerSource }>("meeting-dropped", (e) => setPieces((p) => applyDropped(p, e.payload.source))),
    ];
    return () => {
      for (const o of offs) void o.then((f) => f());
    };
  }, []);

  useEffect(() => {
    speedRef.current = cfg.speed_px_s;
  }, [cfg.speed_px_s]);

  // Measure each piece once per text; the strip never re-lays out per frame.
  useLayoutEffect(() => {
    piecesRef.current = pieces;
    const strip = stripRef.current;
    if (!strip) return;
    const next: Record<string, number> = {};
    for (const el of Array.from(strip.children) as HTMLElement[]) {
      const id = el.dataset.id;
      if (id) next[id] = el.offsetWidth;
    }
    widths.current = next;
  }, [pieces, cfg.font_px]);

  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = now - last;
      last = now;
      const box = boxRef.current;
      const strip = stripRef.current;
      if (box && strip) {
        const view = box.clientWidth;
        const anchor = view - REST_GAP;
        const total = piecesRef.current.reduce((s, p) => s + (widths.current[p.id] ?? 0), 0);
        if (offset.current === null) offset.current = anchor;
        // New text enters from the right edge.
        if (offset.current + total < anchor) offset.current = anchor - total;
        const end = offset.current + total;
        const move = reduce ? end - anchor : advance(end, anchor, dt, speedRef.current, view);
        offset.current -= move;
        const pruned = prune(piecesRef.current, widths.current, offset.current);
        if (pruned.pieces !== piecesRef.current) {
          offset.current = pruned.offset;
          piecesRef.current = pruned.pieces;
          setPieces(pruned.pieces);
        }
        strip.style.transform = `translate3d(${offset.current}px,0,0)`;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div
      ref={boxRef}
      className="anim-keep"
      style={{
        position: "fixed",
        inset: 0,
        overflow: "hidden",
        borderRadius: 10,
        background: `rgba(18, 18, 22, ${cfg.opacity})`,
        display: "flex",
        alignItems: "center",
        fontFamily: "-apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif",
        fontSize: cfg.font_px,
        lineHeight: 1.2,
        pointerEvents: "none",
        userSelect: "none",
      }}
    >
      <div ref={stripRef} style={{ display: "flex", whiteSpace: "nowrap", alignItems: "center" }}>
        {pieces.map((p, i) => (
          <span
            key={p.id}
            data-id={p.id}
            style={{ paddingRight: "0.9em", display: "inline-flex", alignItems: "center", gap: "0.45em" }}
          >
            {showsLabel(pieces, i) && (
              <span
                style={{
                  fontSize: "0.68em",
                  fontWeight: 700,
                  letterSpacing: "0.06em",
                  color: "#111",
                  background: ACCENT[p.source],
                  borderRadius: 4,
                  padding: "0.1em 0.4em",
                }}
              >
                {p.source.toUpperCase()}
              </span>
            )}
            <span
              style={{
                color: p.final ? "#f5f5f5" : "#9a9a9a",
                fontStyle: p.final ? "normal" : "italic",
                borderLeft: showsLabel(pieces, i) ? undefined : `2px solid ${ACCENT[p.source]}33`,
                paddingLeft: showsLabel(pieces, i) ? undefined : "0.4em",
              }}
            >
              {p.text}
            </span>
          </span>
        ))}
      </div>
    </div>
  );
}
