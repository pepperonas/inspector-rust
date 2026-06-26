/**
 * `window-palette` overlay — the Moom-style hover palette (macOS). Anchored
 * under a window's green zoom button by the Rust hover monitor; here we render
 * the preset row + the hex grid and report the chosen 0..1 screen-fraction back
 * (`window_palette_apply`), which Rust maps to an absolute rect + applies to the
 * hovered window via the Accessibility API.
 *
 * The geometry is the pure, unit-tested `lib/hexgrid.ts`; this component only
 * draws + tracks the drag. The window is reused (hidden/shown), so it re-reads
 * its context on the `window-palette-shown` event, not just on mount.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  windowPaletteApply,
  windowPaletteCancel,
  windowPaletteContext,
  type PaletteContext,
} from "../lib/ipc";
import {
  boundingFraction,
  cellInRange,
  hexCenters,
  hexPolygon,
  nearestCell,
} from "../lib/hexgrid";

interface Cell {
  col: number;
  row: number;
}

interface Preset {
  key: string;
  label: string;
  frac: [number, number, number, number]; // x, y, w, h in 0..1
}

const HALVES: Preset[] = [
  { key: "max", label: "Maximize", frac: [0, 0, 1, 1] },
  { key: "left", label: "Left half", frac: [0, 0, 0.5, 1] },
  { key: "right", label: "Right half", frac: [0.5, 0, 0.5, 1] },
  { key: "top", label: "Top half", frac: [0, 0, 1, 0.5] },
  { key: "bottom", label: "Bottom half", frac: [0, 0.5, 1, 0.5] },
];

const QUARTERS: Preset[] = [
  { key: "max", label: "Maximize", frac: [0, 0, 1, 1] },
  { key: "tl", label: "Top-left", frac: [0, 0, 0.5, 0.5] },
  { key: "tr", label: "Top-right", frac: [0.5, 0, 0.5, 0.5] },
  { key: "bl", label: "Bottom-left", frac: [0, 0.5, 0.5, 0.5] },
  { key: "br", label: "Bottom-right", frac: [0.5, 0.5, 0.5, 0.5] },
];

export function WindowPalette() {
  const [ctx, setCtx] = useState<PaletteContext | null>(null);
  const [optionDown, setOptionDown] = useState(false);
  const [drag, setDrag] = useState<{ a: Cell; b: Cell } | null>(null);
  const [showSeq, setShowSeq] = useState(0); // bumps each show → replays entrance
  const svgRef = useRef<SVGSVGElement>(null);
  const draggingRef = useRef(false);

  const reload = useCallback(() => {
    windowPaletteContext()
      .then(setCtx)
      .catch(() => {});
    setDrag(null);
    draggingRef.current = false;
    setShowSeq((s) => s + 1);
  }, []);

  useEffect(() => {
    reload();
    const un = listen("window-palette-shown", () => reload());
    return () => {
      void un.then((f) => f());
    };
  }, [reload]);

  // Option toggles the preset row between halves and quarters.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === "Alt") setOptionDown(true);
      if (e.key === "Escape") void windowPaletteCancel();
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === "Alt") setOptionDown(false);
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);

  const cols = ctx?.cols ?? 8;
  const rows = ctx?.rows ?? 6;

  // Grid box at the honeycomb's *natural* aspect so pointy-top cells stay
  // regular (rendered with preserveAspectRatio="meet", not stretched). For a
  // regular hex hexHeight = wHex·2/√3, so:
  //   boxH/boxW = (2/√3)·(0.75·rows + 0.25) / (cols + 0.5)
  const boxW = 284;
  const boxH = boxW * ((2 / Math.sqrt(3)) * (0.75 * rows + 0.25)) / (cols + 0.5);

  const cells = useMemo(() => hexCenters(cols, rows, boxW, boxH), [cols, rows, boxW, boxH]);
  const wHex = boxW / (cols + 0.5);
  const hHex = boxH / (0.75 * rows + 0.25);
  // Pointy-top: flat-to-flat width = √3·rx, point-to-point height = 2·ry. Touch
  // at wHex / hHex; ×0.94 leaves the thin honeycomb gap.
  const cellRx = (wHex / Math.sqrt(3)) * 0.94;
  const cellRy = (hHex / 2) * 0.94;

  const presets = optionDown ? QUARTERS : HALVES;

  const apply = (frac: [number, number, number, number]) => {
    void windowPaletteApply(frac[0], frac[1], frac[2], frac[3]);
  };

  const cellFromEvent = (e: React.PointerEvent): Cell | null => {
    const svg = svgRef.current;
    if (!svg) return null;
    const r = svg.getBoundingClientRect();
    // The viewBox is fit with preserveAspectRatio="meet" → it's scaled
    // uniformly + centred (letterboxed), so undo that to get viewBox coords.
    const s = Math.min(r.width / boxW, r.height / boxH);
    const offX = (r.width - boxW * s) / 2;
    const offY = (r.height - boxH * s) / 2;
    const px = (e.clientX - r.left - offX) / s;
    const py = (e.clientY - r.top - offY) / s;
    const c = nearestCell(cells, px, py);
    return c ? { col: c.col, row: c.row } : null;
  };

  const onPointerDown = (e: React.PointerEvent) => {
    const c = cellFromEvent(e);
    if (!c) return;
    e.preventDefault();
    (e.target as Element).setPointerCapture?.(e.pointerId);
    draggingRef.current = true;
    setDrag({ a: c, b: c });
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!draggingRef.current) return;
    const c = cellFromEvent(e);
    if (c) setDrag((d) => (d ? { a: d.a, b: c } : null));
  };
  const onPointerUp = () => {
    if (!draggingRef.current || !drag) {
      draggingRef.current = false;
      return;
    }
    draggingRef.current = false;
    const f = boundingFraction(drag.a, drag.b, cols, rows);
    apply([f.x, f.y, f.w, f.h]);
  };

  return (
    <div
      key={showSeq}
      className="wp-pop flex h-screen w-screen flex-col gap-2 rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]/95 p-2.5 text-[var(--color-fg)] shadow-2xl backdrop-blur"
    >
      {/* Preset row */}
      <div className="flex items-center justify-between gap-1">
        {presets.map((p) => (
          <button
            key={p.key}
            type="button"
            title={p.label}
            onClick={() => apply(p.frac)}
            className="group flex h-9 flex-1 items-center justify-center rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] transition-colors hover:border-[var(--color-accent)]"
          >
            <PresetGlyph frac={p.frac} />
          </button>
        ))}
      </div>

      {/* Hex grid */}
      <svg
        ref={svgRef}
        viewBox={`0 0 ${boxW} ${boxH}`}
        preserveAspectRatio="xMidYMid meet"
        className="w-full flex-1 touch-none select-none"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        {cells.map((c) => {
          const on = drag ? cellInRange(c.col, c.row, drag.a, drag.b) : false;
          return (
            <polygon
              key={`${c.col}-${c.row}`}
              className="wp-cell"
              points={hexPolygon(c.cx, c.cy, cellRx, cellRy)}
              fill={on ? "var(--color-accent)" : "var(--color-bg)"}
              fillOpacity={on ? 0.9 : 0.45}
              stroke={on ? "var(--color-accent)" : "var(--color-border)"}
              strokeWidth={1}
              vectorEffect="non-scaling-stroke"
            />
          );
        })}
      </svg>

      <p className="text-center text-[9px] text-[var(--color-muted)]">
        drag to snap · ⌥ quarters · Esc cancel
      </p>
    </div>
  );
}

/** A tiny rectangle-in-a-box icon depicting a preset's region. */
function PresetGlyph({ frac }: { frac: [number, number, number, number] }) {
  const W = 20;
  const H = 14;
  return (
    <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} className="opacity-80 group-hover:opacity-100">
      <rect x={0.5} y={0.5} width={W - 1} height={H - 1} rx={2} fill="none" stroke="var(--color-muted)" strokeWidth={1} />
      <rect
        x={frac[0] * W}
        y={frac[1] * H}
        width={frac[2] * W}
        height={frac[3] * H}
        rx={1}
        fill="var(--color-accent)"
      />
    </svg>
  );
}
