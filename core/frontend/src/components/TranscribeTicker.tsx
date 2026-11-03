import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { GripVertical, LocateFixed, Maximize2, Minimize2, Minus, X } from "lucide-react";
import { getMeetingConfig, tickerUpdate, type TickerConfig } from "../lib/ipc";
import { useT } from "../lib/i18n";
import {
  advance,
  applyDropped,
  applyFinal,
  applyHypothesis,
  lastLines,
  nextMode,
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
  mode: "bar",
  place: null,
};

/** Lines in the maximised view (mirrors `ticker::MAX_LINES`). */
const MAX_LINES = 5;
/** After a drag: wait this long without moves before saving the place. */
const SAVE_PLACE_MS = 400;

/**
 * The transcription ticker (its own transparent, click-through window).
 * Finals in light text, previews grey and italic; a preview is replaced by
 * its final in place. One element moves, by transform only, in one rAF loop.
 */
export function TranscribeTicker() {
  const t = useT();
  const [cfg, setCfg] = useState<TickerConfig>(DEFAULTS);
  // The window is click-through; Rust makes it take clicks while the pointer
  // is over it and tells us, so the controls show only then.
  const [hover, setHover] = useState(false);
  const dragging = useRef(false);
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
      listen<boolean>("meeting-ticker-hover", (e) => setHover(e.payload)),
    ];
    // Remember the place once a drag has come to rest.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const moved = getCurrentWindow().onMoved(() => {
      if (!dragging.current) return;
      clearTimeout(timer);
      timer = setTimeout(() => {
        dragging.current = false;
        void tickerUpdate({ save_place: true }).then((c) => setCfg(c.ticker)).catch(() => {});
      }, SAVE_PLACE_MS);
    });
    offs.push(moved as unknown as (typeof offs)[number]);
    return () => {
      clearTimeout(timer);
      for (const o of offs) void o.then((f) => f());
    };
  }, []);

  const modeRef = useRef(cfg.mode);
  useEffect(() => {
    modeRef.current = cfg.mode;
  }, [cfg.mode]);

  const patch = (p: Parameters<typeof tickerUpdate>[0]) =>
    void tickerUpdate(p)
      .then((c) => setCfg(c.ticker))
      .catch(() => {});

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
      if (box && strip && modeRef.current !== "max") {
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

  const label = (p: Piece) => (
    <span
      style={{
        fontSize: "0.68em",
        fontWeight: 700,
        letterSpacing: "0.06em",
        color: "#111",
        background: ACCENT[p.source],
        borderRadius: 4,
        padding: "0.1em 0.4em",
        flexShrink: 0,
      }}
    >
      {p.source.toUpperCase()}
    </span>
  );
  const textStyle = (p: Piece) => ({
    color: p.final ? "#f5f5f5" : "#9a9a9a",
    fontStyle: p.final ? ("normal" as const) : ("italic" as const),
  });
  const btn: React.CSSProperties = {
    width: 22,
    height: 22,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 6,
    border: "none",
    background: "rgba(255,255,255,0.08)",
    color: "#e5e5e5",
    cursor: "pointer",
    padding: 0,
  };
  const max = cfg.mode === "max";

  return (
    <div
      ref={boxRef}
      className="anim-keep"
      data-testid="ticker"
      style={{
        position: "fixed",
        inset: 0,
        overflow: "hidden",
        borderRadius: 10,
        background: `rgba(18, 18, 22, ${cfg.opacity})`,
        display: "flex",
        alignItems: max ? "flex-end" : "center",
        fontFamily: "-apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif",
        fontSize: cfg.font_px,
        lineHeight: 1.2,
        userSelect: "none",
      }}
    >
      {max ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "8px 14px", width: "100%" }}>
          {lastLines(pieces, MAX_LINES).map((p) => (
            <div key={p.id} data-id={p.id} style={{ display: "flex", gap: "0.5em", alignItems: "baseline", lineHeight: 1.35 }}>
              {label(p)}
              <span style={{ ...textStyle(p), overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.text}</span>
            </div>
          ))}
        </div>
      ) : (
        <div ref={stripRef} style={{ display: "flex", whiteSpace: "nowrap", alignItems: "center", pointerEvents: "none" }}>
          {pieces.map((p, i) => (
            <span
              key={p.id}
              data-id={p.id}
              style={{ paddingRight: "0.9em", display: "inline-flex", alignItems: "center", gap: "0.45em" }}
            >
              {showsLabel(pieces, i) && label(p)}
              <span
                style={{
                  ...textStyle(p),
                  borderLeft: showsLabel(pieces, i) ? undefined : `2px solid ${ACCENT[p.source]}33`,
                  paddingLeft: showsLabel(pieces, i) ? undefined : "0.4em",
                }}
              >
                {p.text}
              </span>
            </span>
          ))}
        </div>
      )}
      {hover && (
        <div
          data-testid="ticker-controls"
          style={{
            position: "absolute",
            top: 0,
            bottom: 0,
            left: 0,
            display: "flex",
            alignItems: max ? "flex-start" : "center",
            gap: 4,
            padding: max ? "6px 10px" : "0 10px",
            background: `linear-gradient(90deg, rgba(18,18,22,0.98) 75%, rgba(18,18,22,0))`,
            paddingRight: 24,
          }}
        >
          <button
            type="button"
            title={t("tick.drag")}
            aria-label={t("tick.drag")}
            data-testid="ticker-drag"
            style={{ ...btn, cursor: "grab" }}
            onPointerDown={(e) => {
              e.preventDefault();
              dragging.current = true;
              void getCurrentWindow().startDragging();
            }}
          >
            <GripVertical size={14} />
          </button>
          <button
            type="button"
            title={cfg.mode === "mini" ? t("tick.restore") : t("tick.mini")}
            aria-label={cfg.mode === "mini" ? t("tick.restore") : t("tick.mini")}
            data-testid="ticker-mini"
            style={btn}
            onClick={() => patch({ mode: nextMode(cfg.mode, "mini") })}
          >
            <Minus size={14} />
          </button>
          <button
            type="button"
            title={max ? t("tick.restore") : t("tick.max")}
            aria-label={max ? t("tick.restore") : t("tick.max")}
            data-testid="ticker-max"
            style={btn}
            onClick={() => patch({ mode: nextMode(cfg.mode, "max") })}
          >
            {max ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
          </button>
          {cfg.place && (
            <button
              type="button"
              title={t("tick.dock")}
              aria-label={t("tick.dock")}
              data-testid="ticker-dock"
              style={btn}
              onClick={() => patch({ reset_place: true })}
            >
              <LocateFixed size={13} />
            </button>
          )}
          <button
            type="button"
            title={t("tick.off")}
            aria-label={t("tick.off")}
            data-testid="ticker-off"
            style={btn}
            onClick={() => patch({ visible: false })}
          >
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
