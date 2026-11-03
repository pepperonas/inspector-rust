import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { recordRegionCancel, recordRegionChosen } from "../lib/ipc";
import {
  fromDrag,
  handleCursor,
  handlePoint,
  HANDLES,
  isBigEnough,
  moveRect,
  resizeRect,
  roundRect,
  sizeLabel,
  toolbarTop,
  type Handle,
  type RegionRect,
} from "../lib/record-region";
import { useT } from "../lib/i18n";

/** Height of the button row (px) — `toolbarTop` places it from this. */
const ROW_H = 36;
/** Rough width of the button row, to keep it on screen at the right edge. */
const TOOLBAR_W = 560;

type Drag =
  | { kind: "draw"; start: { x: number; y: number } }
  | { kind: "move"; start: { x: number; y: number }; from: RegionRect }
  | { kind: "resize"; start: { x: number; y: number }; from: RegionRect; handle: Handle };

const bounds = () => ({ w: window.innerWidth, h: window.innerHeight });

/**
 * Full-screen free-area picker (window `record-region`): drag a rectangle, adjust it with the
 * eight handles or move it by its body, Enter / "Use area" confirms, Esc cancels. The window
 * covers exactly one display, so its CSS pixels are that display's points.
 */
export function RecordRegionPicker() {
  const t = useT();
  const [rect, setRect] = useState<RegionRect | null>(null);
  const [busy, setBusy] = useState(false);
  const drag = useRef<Drag | null>(null);

  const cancel = useCallback(() => void recordRegionCancel().catch(() => undefined), []);

  const confirm = useCallback(() => {
    if (!isBigEnough(rect) || busy) return;
    setBusy(true);
    recordRegionChosen(roundRect(rect, window.devicePixelRatio || 1, bounds())).catch(() => setBusy(false));
  }, [rect, busy]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      } else if (e.key === "Enter" && !e.isComposing) {
        e.preventDefault();
        confirm();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel, confirm]);

  // A reused overlay (opened again while the old one was still closing) starts empty.
  useEffect(() => {
    let off: (() => void) | undefined;
    let dead = false;
    void listen("record-region-reset", () => {
      drag.current = null;
      setRect(null);
      setBusy(false);
    }).then((u) => (dead ? u() : (off = u)));
    return () => {
      dead = true;
      off?.();
    };
  }, []);

  const point = (e: React.PointerEvent) => ({ x: e.clientX, y: e.clientY });

  const begin = (e: React.PointerEvent, d: Drag) => {
    if (e.button !== 0 || busy) return;
    e.preventDefault();
    e.stopPropagation();
    drag.current = d;
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
  };

  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const p = point(e);
    const dx = p.x - d.start.x;
    const dy = p.y - d.start.y;
    if (d.kind === "draw") setRect(fromDrag(d.start, p, bounds()));
    else if (d.kind === "move") setRect(moveRect(d.from, dx, dy, bounds()));
    else setRect(resizeRect(d.from, d.handle, dx, dy, bounds()));
  };

  const onUp = () => {
    const d = drag.current;
    drag.current = null;
    // A first drag that stayed too small was a click: start over.
    if (d?.kind === "draw") setRect((r) => (isBigEnough(r) ? r : null));
  };

  const ok = isBigEnough(rect);
  const showRect = rect && rect.w > 0 && rect.h > 0;

  return (
    <div
      data-testid="rec-region-root"
      className="relative h-screen w-screen select-none overflow-hidden"
      style={{ cursor: "crosshair" }}
      onPointerDown={(e) => begin(e, { kind: "draw", start: point(e) })}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={onUp}
    >
      {showRect ? (
        <div
          data-testid="rec-region-rect"
          className="absolute border border-[var(--color-accent)]"
          style={{
            left: rect.x,
            top: rect.y,
            width: rect.w,
            height: rect.h,
            boxShadow: "0 0 0 99999px rgba(0,0,0,0.35)",
            cursor: ok ? "move" : "crosshair",
          }}
          onPointerDown={(e) => {
            if (!ok) return;
            begin(e, { kind: "move", start: point(e), from: rect });
          }}
        />
      ) : (
        <div className="pointer-events-none absolute inset-0" style={{ background: "rgba(0,0,0,0.35)" }} />
      )}

      {showRect && (
        <div
          className="pointer-events-none absolute rounded-md bg-black/75 px-2 py-0.5 text-[12px] font-medium tabular-nums text-white"
          style={{ left: rect.x, top: rect.y >= 28 ? rect.y - 26 : rect.y + 6 }}
          data-testid="rec-region-size"
        >
          {sizeLabel(rect)}
        </div>
      )}

      {ok &&
        HANDLES.map((h) => {
          const p = handlePoint(rect, h);
          return (
            <div
              key={h}
              data-testid={`rec-region-handle-${h}`}
              className="absolute h-2.5 w-2.5 rounded-[2px] border border-[var(--color-accent)] bg-white"
              style={{ left: p.x - 5, top: p.y - 5, cursor: handleCursor(h) }}
              onPointerDown={(e) => begin(e, { kind: "resize", start: point(e), from: rect, handle: h })}
            />
          );
        })}

      {!rect && (
        <div className="pointer-events-none absolute left-1/2 top-10 -translate-x-1/2 rounded-lg bg-black/75 px-4 py-2 text-[13px] text-white shadow-lg">
          {t("rec2.region.hint")}
        </div>
      )}

      {ok && (
        <div
          className="absolute flex items-center gap-2"
          style={{
            left: Math.max(0, Math.min(rect.x, window.innerWidth - TOOLBAR_W)),
            top: toolbarTop(rect, bounds(), ROW_H),
            height: ROW_H,
          }}
          onPointerDown={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={confirm}
            disabled={busy}
            className="rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[13px] font-medium text-[var(--color-accent-fg)] shadow-lg disabled:opacity-60"
          >
            {t("rec2.region.use")}
          </button>
          <button
            type="button"
            onClick={() => {
              setRect(null);
              setBusy(false);
            }}
            className="rounded-lg bg-black/75 px-3 py-1.5 text-[13px] text-white shadow-lg"
          >
            {t("rec2.region.redo")}
          </button>
          <button
            type="button"
            onClick={cancel}
            className="rounded-lg bg-black/75 px-3 py-1.5 text-[13px] text-white shadow-lg"
          >
            {t("rec2.region.cancel")}
          </button>
          <span className="rounded-md bg-black/60 px-2 py-1 text-[11px] text-white/85">{t("rec2.region.hint_adjust")}</span>
        </div>
      )}
    </div>
  );
}

/**
 * Dashed frame around the area being recorded (window `record-region-frame`, click-through,
 * excluded from the capture). The window is the area plus 4 pt on every side; the line sits
 * in that margin, so it never covers a recorded pixel.
 */
export function RecordRegionFrame() {
  return (
    <div className="h-screen w-screen bg-transparent">
      <div className="absolute" style={{ inset: 1, border: "2px dashed rgba(239,68,68,0.95)", borderRadius: 2 }} />
    </div>
  );
}
