import { useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import type { RecordCropRect } from "../../lib/ipc";
import { MIN_FRAMING_SIZE, cropFromFraming, framingFromCrop } from "../../lib/camera-layout";
import { RangeSlider, chipClass } from "./InspectorParts";
import { useT } from "../../lib/i18n";

const PAN_STEP = 0.05;
const ZOOM_STEP = 1.1;

/**
 * The camera's framing: a still of the camera file with the kept frame laid over it. Dragging the
 * frame pans, the slider (and + / −) zooms; arrow keys move it. The frame keeps the camera's
 * shape — the mask above cuts it afterwards — so a corner never changes more than the size.
 * Pan lives here (0…1 per axis), not in the rect: at 100 % the rect cannot remember it.
 */
export function CameraFraming({
  src,
  crop,
  disabled,
  onLive,
  onEnd,
  onCommit,
}: {
  /** The camera file (asset URL); `null` = no picture to show. */
  src: string | null;
  crop: RecordCropRect | null;
  disabled?: boolean;
  onLive: (crop: RecordCropRect | null) => void;
  onEnd: () => void;
  onCommit: (crop: RecordCropRect | null) => void;
}) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const [aspect, setAspect] = useState(16 / 9);
  const [size, setSize] = useState(() => framingFromCrop(crop).size);
  const [pan, setPan] = useState(() => framingFromCrop(crop).pan);
  // A crop from outside (undo, a preset): follow it (derived during render).
  const [seen, setSeen] = useState(crop);
  const [mine, setMine] = useState<RecordCropRect | null | undefined>(undefined);
  if (crop !== seen) {
    setSeen(crop);
    if (JSON.stringify(crop) !== JSON.stringify(mine)) {
      const f = framingFromCrop(crop);
      setSize(f.size);
      if (crop) setPan(f.pan);
    }
  }
  const drag = useRef<{ x0: number; y0: number; pan: { x: number; y: number } } | null>(null);

  const emit = (s: number, p: { x: number; y: number }, commit: boolean) => {
    setSize(s);
    setPan(p);
    const c = cropFromFraming(s, p);
    setMine(c);
    if (commit) onCommit(c);
    else onLive(c);
  };
  const shown = cropFromFraming(size, pan) ?? { x: 0, y: 0, w: 1, h: 1 };

  const onDown = (e: ReactPointerEvent) => {
    if (disabled) return;
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* synthetic events */
    }
    drag.current = { x0: e.clientX, y0: e.clientY, pan };
  };
  const onMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    const r = box.current?.getBoundingClientRect();
    if (!d || !r || size >= 1 || !r.width || !r.height) return;
    const free = 1 - size;
    const c = (v: number) => Math.min(1, Math.max(0, v));
    emit(size, { x: c(d.pan.x + (e.clientX - d.x0) / (r.width * free)), y: c(d.pan.y + (e.clientY - d.y0) / (r.height * free)) }, false);
  };
  const onUp = () => {
    if (!drag.current) return;
    drag.current = null;
    onEnd();
  };
  const onKey = (e: KeyboardEvent) => {
    if (disabled) return;
    const c = (v: number) => Math.min(1, Math.max(0, v));
    const moves: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    const m = moves[e.key];
    if (m && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      if (size < 1) emit(size, { x: c(pan.x + m[0] * PAN_STEP), y: c(pan.y + m[1] * PAN_STEP) }, true);
    } else if (e.key === "+" || e.key === "=" || e.key === "-") {
      e.preventDefault();
      const next = e.key === "-" ? Math.min(1, size * ZOOM_STEP) : Math.max(MIN_FRAMING_SIZE, size / ZOOM_STEP);
      emit(next, pan, true);
    }
  };

  return (
    <div className="space-y-2">
      <div
        ref={box}
        tabIndex={disabled ? -1 : 0}
        role="group"
        aria-label={t("rec5.cam.framing")}
        title={t("rec5.cam.framing_hint")}
        onKeyDown={onKey}
        className="relative w-full overflow-hidden rounded-md bg-black outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]"
        style={{ aspectRatio: String(aspect) }}
        data-testid="ed-camera-framing"
      >
        {src ? (
          <video
            src={src}
            muted
            playsInline
            preload="auto"
            onLoadedMetadata={(e) => {
              const v = e.currentTarget;
              if (v.videoWidth > 0 && v.videoHeight > 0) setAspect(v.videoWidth / v.videoHeight);
              try {
                v.currentTime = Math.min(0.5, (v.duration || 1) / 2);
              } catch {
                /* not seekable yet */
              }
            }}
            className="pointer-events-none absolute inset-0 h-full w-full object-fill"
          />
        ) : (
          <p className="absolute inset-0 flex items-center justify-center px-2 text-center text-[11px] text-white/70">
            {t("rec5.cam.framing_no_still")}
          </p>
        )}
        <div
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
          className={"absolute rounded-sm border-2 border-white " + (disabled || size >= 1 ? "" : "cursor-grab active:cursor-grabbing")}
          style={{
            left: `${shown.x * 100}%`,
            top: `${shown.y * 100}%`,
            width: `${shown.w * 100}%`,
            height: `${shown.h * 100}%`,
            boxShadow: "0 0 0 9999px rgba(0,0,0,0.55)",
          }}
          data-testid="ed-camera-frame"
        />
      </div>
      <RangeSlider
        label={t("rec5.cam.framing_zoom")}
        min={100}
        max={Math.round(100 / MIN_FRAMING_SIZE)}
        step={1}
        value={Math.round(100 / size)}
        format={(v) => `${Math.round(v)} %`}
        onLive={(v) => emit(Math.min(1, Math.max(MIN_FRAMING_SIZE, 100 / v)), pan, false)}
        onEnd={onEnd}
      />
      <button type="button" className={chipClass(false)} disabled={size >= 1} onClick={() => emit(1, { x: 0.5, y: 0.5 }, true)}>
        {t("rec5.cam.framing_reset")}
      </button>
      <p className="text-[11px] text-[var(--color-muted)]">{t("rec5.cam.framing_hint")}</p>
    </div>
  );
}
