import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { RecordAnnotation } from "../../lib/ipc";
import {
  BOX_HANDLES,
  annotationOutputRect,
  dragAnnotationBox,
  insertFreehandPoint,
  isTypingTarget,
  moveFreehandPoint,
  removeFreehandPoint,
  type AnnBox,
  type BoxHandle,
} from "../../lib/editor";
import { useT, type MessageKey } from "../../lib/i18n";

/** What a gesture on the overlay changes: the box (percent) or the free-form outline. */
export type AnnotationPatch = Partial<AnnBox> & { points?: [number, number][] };

/** A pointer moved less than this many px: a click, not a drag. */
const CLICK_SLOP_PX = 3;

const HANDLE_POS: Record<Exclude<BoxHandle, "move">, { left: string; top: string; cursor: string }> = {
  nw: { left: "0%", top: "0%", cursor: "nwse-resize" },
  n: { left: "50%", top: "0%", cursor: "ns-resize" },
  ne: { left: "100%", top: "0%", cursor: "nesw-resize" },
  e: { left: "100%", top: "50%", cursor: "ew-resize" },
  se: { left: "100%", top: "100%", cursor: "nwse-resize" },
  s: { left: "50%", top: "100%", cursor: "ns-resize" },
  sw: { left: "0%", top: "100%", cursor: "nesw-resize" },
  w: { left: "0%", top: "50%", cursor: "ew-resize" },
};

type Gesture =
  | { kind: "box"; handle: BoxHandle; x0: number; y0: number; start: AnnBox; moved: boolean }
  | { kind: "point"; index: number; moved: boolean };

/**
 * Selection chrome of the selected annotation over the preview picture (the pixels are the
 * compositor's): a frame, dragging the body, 8 size handles. Text, image and arrow boxes are
 * percent of the output frame; a blur box is percent of the clip's screen box (`screenRect`,
 * Rust's layout). Under an active zoom a blur moves with the zoomed content, so its handles stay
 * off and a note says why. A free-form blur also shows its outline: a click inside the box adds
 * a point, dragging a point moves it, Delete removes the selected point (at least 3 remain).
 *
 * Every drag reports live (`onLive`) and ends once (`onEnd`: one undo step); a click that adds
 * or a key that removes a point is one step (`onCommit`).
 */
export function AnnotationOverlay({
  annotation,
  screenRect,
  zoomActive,
  visible = true,
  onLive,
  onEnd,
  onCommit,
}: {
  annotation: RecordAnnotation;
  /** The clip's screen box in the output `[x, y, w, h]` (fractions); a blur's anchor. */
  screenRect: readonly [number, number, number, number] | null;
  /** A zoom of the annotation's clip covers the playhead. */
  zoomActive: boolean;
  /**
   * The playhead lies inside the annotation's time span (Rust's programme span). Outside it the
   * compositor draws nothing there, so the frame shows dimmed and cannot be edited.
   */
  visible?: boolean;
  onLive: (patch: AnnotationPatch) => void;
  onEnd: () => void;
  onCommit: (patch: AnnotationPatch) => void;
}) {
  const t = useT();
  const root = useRef<HTMLDivElement>(null);
  const [gesture, setGesture] = useState<Gesture | null>(null);
  const [point, setPoint] = useState<number | null>(null);
  const blur = annotation.body.kind === "blur" ? annotation.body : null;
  const freehand = blur?.shape === "freehand" ? blur.points : null;
  const anchor: readonly [number, number, number, number] = blur && screenRect ? screenRect : [0, 0, 1, 1];
  const [rx, ry, rw, rh] = annotationOutputRect(annotation, screenRect);

  // Delete removes the selected outline point. Capture phase: it runs before the editor's and
  // the timeline's window handlers, which would otherwise delete the whole annotation or clip.
  const live = useRef({ point, freehand, onCommit, visible });
  useEffect(() => {
    live.current = { point, freehand, onCommit, visible };
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cur = live.current;
      if (e.key !== "Delete" && e.key !== "Backspace") return;
      if (cur.point == null || !cur.freehand || !cur.visible || isTypingTarget(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      const next = removeFreehandPoint(cur.freehand, cur.point);
      setPoint(null);
      if (next.length !== cur.freehand.length) cur.onCommit({ points: next });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  if (blur && zoomActive) {
    return (
      <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center" data-testid="ed-ann-zoom-note">
        <p className="max-w-[80%] rounded-md bg-black/70 px-2 py-1 text-center text-[11px] text-white">{t("rec4.ann.blur_zoom_note")}</p>
      </div>
    );
  }

  const pct = (v: number) => `${v * 100}%`;
  if (!visible) {
    return (
      <div className="pointer-events-none absolute inset-0" data-testid="ed-ann-overlay">
        <div
          className="absolute border-2 border-dashed border-white/50"
          style={{ left: pct(rx), top: pct(ry), width: pct(rw), height: pct(rh) }}
          data-testid="ed-ann-box-hidden"
        >
          <span className="absolute left-0 top-full mt-1 whitespace-nowrap rounded bg-black/70 px-1.5 py-0.5 text-[10px] text-white">
            {t("rec4.ann.not_at_playhead")}
          </span>
        </div>
      </div>
    );
  }

  /** Root size in px, and how many px one percent of the anchor is. */
  const metrics = () => {
    const r = root.current?.getBoundingClientRect();
    const w = r?.width ?? 0;
    const h = r?.height ?? 0;
    return { r, pxX: (w * anchor[2]) / 100, pxY: (h * anchor[3]) / 100 };
  };
  /** `(clientX, clientY)` in percent of the annotation box. */
  const inBox = (clientX: number, clientY: number): [number, number] | null => {
    const { r } = metrics();
    if (!r || !r.width || !r.height || rw <= 0 || rh <= 0) return null;
    return [((clientX - r.left - rx * r.width) / (rw * r.width)) * 100, ((clientY - r.top - ry * r.height) / (rh * r.height)) * 100];
  };
  const capture = (e: ReactPointerEvent) => {
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* no active pointer (synthetic events) */
    }
  };
  const startBox = (handle: BoxHandle) => (e: ReactPointerEvent) => {
    e.stopPropagation();
    capture(e);
    const { x, y, w, h } = annotation;
    setGesture({ kind: "box", handle, x0: e.clientX, y0: e.clientY, start: { x, y, w, h }, moved: false });
  };

  const onMove = (e: ReactPointerEvent) => {
    const g = gesture;
    if (!g) return;
    if (g.kind === "point") {
      const p = inBox(e.clientX, e.clientY);
      if (!p || !freehand) return;
      setGesture({ ...g, moved: true });
      onLive({ points: moveFreehandPoint(freehand, g.index, p) });
      return;
    }
    const dxPx = e.clientX - g.x0;
    const dyPx = e.clientY - g.y0;
    if (!g.moved && g.handle === "move" && Math.abs(dxPx) < CLICK_SLOP_PX && Math.abs(dyPx) < CLICK_SLOP_PX) return;
    const { pxX, pxY } = metrics();
    if (pxX <= 0 || pxY <= 0) return;
    if (!g.moved) setGesture({ ...g, moved: true });
    onLive(dragAnnotationBox(g.start, g.handle, dxPx / pxX, dyPx / pxY));
  };

  const onUp = (e: ReactPointerEvent) => {
    const g = gesture;
    setGesture(null);
    if (!g) return;
    if (g.moved) {
      onEnd();
      return;
    }
    // A click on the body of a free-form blur adds an outline point there.
    if (g.kind === "box" && g.handle === "move" && freehand) {
      const p = inBox(e.clientX, e.clientY);
      if (p) {
        const next = insertFreehandPoint(freehand, p);
        setPoint(next.findIndex(([x, y], i) => i >= freehand.length || x !== freehand[i][0] || y !== freehand[i][1]));
        onCommit({ points: next });
      }
    }
  };

  return (
    <div
      ref={root}
      className="pointer-events-none absolute inset-0"
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={() => setGesture(null)}
      data-testid="ed-ann-overlay"
    >
      <div
        role="group"
        aria-label={t("rec4.ann.box")}
        onPointerDown={startBox("move")}
        className="pointer-events-auto absolute cursor-move border-2 border-[var(--color-accent)] bg-[var(--color-accent)]/10"
        style={{ left: pct(rx), top: pct(ry), width: pct(rw), height: pct(rh) }}
        data-testid="ed-ann-box"
      >
        {freehand && (
          <>
            <svg className="pointer-events-none absolute inset-0 h-full w-full overflow-visible" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden>
              <polygon
                points={freehand.map(([x, y]) => `${x},${y}`).join(" ")}
                fill="none"
                stroke="white"
                strokeWidth={1.5}
                strokeDasharray="4 3"
                vectorEffect="non-scaling-stroke"
              />
            </svg>
            {freehand.map(([x, y], i) => (
              <span
                key={i}
                role="button"
                aria-label={t("rec4.ann.point_n", { n: i + 1 })}
                aria-pressed={point === i}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  capture(e);
                  setPoint(i);
                  setGesture({ kind: "point", index: i, moved: false });
                }}
                className={
                  "absolute h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 cursor-grab rounded-full border-2 " +
                  (point === i ? "border-white bg-[var(--color-accent)]" : "border-[var(--color-accent)] bg-white")
                }
                style={{ left: `${x}%`, top: `${y}%` }}
                data-testid="ed-ann-point"
              />
            ))}
          </>
        )}
        {BOX_HANDLES.map((h) => (
          <span
            key={h}
            aria-label={t(`rec4.ann.handle_${h}` as MessageKey)}
            onPointerDown={startBox(h)}
            className="absolute h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-[var(--color-accent)] bg-white"
            style={{ left: HANDLE_POS[h].left, top: HANDLE_POS[h].top, cursor: HANDLE_POS[h].cursor }}
            data-testid={`ed-ann-handle-${h}`}
          />
        ))}
      </div>
    </div>
  );
}
