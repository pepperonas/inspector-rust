import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { CAM_CORNERS, moveCamera, resizeCamera, type CamCorner, type CamRect } from "../../lib/camera-drag";
import { useT, type MessageKey } from "../../lib/i18n";

/** A pointer moved less than this many px: a click, not a drag. */
const CLICK_SLOP_PX = 3;

const CORNER_STYLE: Record<CamCorner, { left: string; top: string; cursor: string }> = {
  nw: { left: "0%", top: "0%", cursor: "nwse-resize" },
  ne: { left: "100%", top: "0%", cursor: "nesw-resize" },
  sw: { left: "0%", top: "100%", cursor: "nesw-resize" },
  se: { left: "100%", top: "100%", cursor: "nwse-resize" },
};

/** What a drag changes: the free position (box centre, fractions of the output), and the size. */
export interface CameraPatch {
  position: { x: number; y: number };
  sizePct?: number;
}

type Gesture = {
  handle: "move" | CamCorner;
  x0: number;
  y0: number;
  start: CamRect;
  /** The size the start box was laid out at (the live size moves with the drag). */
  size: number;
  moved: boolean;
};

/**
 * The picture-in-picture camera over the preview picture (`rect`: Rust's layout, fractions of the
 * output): dragging the body moves it, a corner handle resizes it with the opposite corner fixed.
 * Only the box and its handles take the pointer; the rest of the preview stays clickable. Each
 * drag reports live (`onLive`) and ends once (`onEnd`: one undo step). While dragging the box is
 * drawn where the drag puts it (Rust's answer arrives a moment later).
 */
export function CameraOverlay({
  rect,
  outSize,
  sizePct,
  onLive,
  onEnd,
}: {
  rect: CamRect;
  outSize: readonly [number, number];
  sizePct: number;
  onLive: (patch: CameraPatch) => void;
  onEnd: () => void;
}) {
  const t = useT();
  const root = useRef<HTMLDivElement>(null);
  const [gesture, setGesture] = useState<Gesture | null>(null);
  const [draft, setDraft] = useState<CamRect | null>(null);
  const [x, y, w, h] = gesture && draft ? draft : rect;

  const start = (handle: Gesture["handle"]) => (e: ReactPointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* no active pointer (synthetic events) */
    }
    setGesture({ handle, x0: e.clientX, y0: e.clientY, start: rect, size: sizePct, moved: false });
    setDraft(null);
  };

  const onMove = (e: ReactPointerEvent) => {
    const g = gesture;
    if (!g) return;
    const r = root.current?.getBoundingClientRect();
    if (!r || !r.width || !r.height) return;
    const dxPx = e.clientX - g.x0;
    const dyPx = e.clientY - g.y0;
    if (!g.moved && Math.abs(dxPx) < CLICK_SLOP_PX && Math.abs(dyPx) < CLICK_SLOP_PX) return;
    if (!g.moved) setGesture({ ...g, moved: true });
    if (g.handle === "move") {
      const p = moveCamera(g.start, dxPx / r.width, dyPx / r.height, outSize);
      setDraft(p.rect);
      onLive({ position: { x: p.x, y: p.y } });
    } else {
      const pointer: [number, number] = [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
      const p = resizeCamera(g.start, g.handle, pointer, g.size, outSize);
      setDraft(p.rect);
      onLive({ position: { x: p.x, y: p.y }, sizePct: p.sizePct });
    }
  };

  const onUp = () => {
    const g = gesture;
    setGesture(null);
    setDraft(null);
    if (g?.moved) onEnd();
  };

  const pct = (v: number) => `${v * 100}%`;
  return (
    <div
      ref={root}
      className="pointer-events-none absolute inset-0"
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={onUp}
      data-testid="ed-camera-overlay"
    >
      <div
        role="group"
        aria-label={t("rec5.cam.overlay")}
        title={t("rec5.cam.overlay")}
        onPointerDown={start("move")}
        className="pointer-events-auto absolute cursor-move touch-none rounded-md border-2 border-[var(--color-accent)] bg-[var(--color-accent)]/10"
        style={{ left: pct(x), top: pct(y), width: pct(w), height: pct(h) }}
        data-testid="ed-camera-box"
      >
        {CAM_CORNERS.map((c) => (
          <span
            key={c}
            role="button"
            aria-label={t(`rec5.cam.handle_${c}` as MessageKey)}
            onPointerDown={start(c)}
            className="absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-[var(--color-accent)] bg-white"
            style={{ left: CORNER_STYLE[c].left, top: CORNER_STYLE[c].top, cursor: CORNER_STYLE[c].cursor }}
            data-testid={`ed-camera-handle-${c}`}
          />
        ))}
      </div>
    </div>
  );
}
