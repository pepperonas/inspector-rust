import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { RecordCropRect } from "../../lib/ipc";
import { ASPECT_RATIO, CROP_PRESETS, fitPreview, type CropPreset } from "../../lib/editor";
import { useT } from "../../lib/i18n";

const MIN_EDGE = 0.05;
type Handle = "move" | "nw" | "ne" | "sw" | "se";

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** Moves/resizes `r` by a normalised delta; a locked ratio (in normalised units) keeps w/h. */
function dragRect(r: RecordCropRect, h: Handle, dx: number, dy: number, lock: number | null): RecordCropRect {
  if (h === "move") {
    return { ...r, x: clamp01(Math.min(r.x + dx, 1 - r.w)), y: clamp01(Math.min(r.y + dy, 1 - r.h)) };
  }
  // The opposite corner stays put.
  const ax = h === "nw" || h === "sw" ? r.x + r.w : r.x;
  const ay = h === "nw" || h === "ne" ? r.y + r.h : r.y;
  let bx = clamp01((h === "nw" || h === "sw" ? r.x : r.x + r.w) + dx);
  let by = clamp01((h === "nw" || h === "ne" ? r.y : r.y + r.h) + dy);
  let w = Math.max(MIN_EDGE, Math.abs(bx - ax));
  let hh = Math.max(MIN_EDGE, Math.abs(by - ay));
  if (lock) {
    hh = w / lock;
    const maxH = by < ay ? ay : 1 - ay;
    if (hh > maxH) {
      hh = maxH;
      w = hh * lock;
    }
  }
  bx = bx < ax ? ax - w : ax + w;
  by = by < ay ? ay - hh : ay + hh;
  const x = clamp01(Math.min(ax, bx));
  const y = clamp01(Math.min(ay, by));
  let fw = Math.min(w, 1 - x);
  let fh = Math.min(hh, 1 - y);
  // Clamping one side must not break a locked ratio: shrink the other to match.
  if (lock) {
    if (fw / fh > lock) fw = fh * lock;
    else fh = fw / lock;
  }
  return { x, y, w: fw, h: fh };
}

/**
 * Crop mode: the uncropped source frame (a paused `<video>` at the playhead) with the crop
 * rect on top — corners resize, the body moves — and the preset chips. Changes are only
 * committed when a drag ends.
 */
export function CropOverlay({
  srcW,
  srcH,
  videoSrc,
  sourceMs,
  rect,
  preset,
  onPreset,
  onCommit,
  onReset,
}: {
  srcW: number;
  srcH: number;
  videoSrc: string;
  sourceMs: number;
  rect: RecordCropRect | null;
  preset: CropPreset;
  onPreset: (p: CropPreset) => void;
  onCommit: (r: RecordCropRect) => void;
  onReset: () => void;
}) {
  const t = useT();
  const panel = useRef<HTMLDivElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [live, setLive] = useState<RecordCropRect | null>(null);
  const [drag, setDrag] = useState<{ h: Handle; x: number; y: number; start: RecordCropRect } | null>(null);
  // The dragged rect stays on screen until the document answers with the new one (no flash
  // back to the old rect between pointer-up and Rust's normalised doc).
  const [seenRect, setSeenRect] = useState(rect);
  if (seenRect !== rect) {
    setSeenRect(rect);
    setLive(null);
  }
  const shown = live ?? rect ?? { x: 0, y: 0, w: 1, h: 1 };
  // Locked ratio in normalised units (w/h of the rect as fractions of the frame).
  const lock = preset === "free" ? null : (ASPECT_RATIO[preset] * srcH) / srcW;

  useEffect(() => {
    const el = panel.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((e) => {
      const r = e[0]?.contentRect;
      if (r) setSize({ w: r.width, h: r.height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const v = video.current;
    if (!v) return;
    try {
      v.currentTime = sourceMs / 1000;
    } catch {
      /* metadata not loaded yet: the loadedmetadata handler sets it */
    }
  }, [sourceMs]);

  const frame = fitPreview(size.w - 24, size.h - 64, srcW / Math.max(1, srcH), Infinity, Infinity);

  const down = (h: Handle) => (e: ReactPointerEvent) => {
    e.stopPropagation();
    (e.target as Element).setPointerCapture?.(e.pointerId);
    setDrag({ h, x: e.clientX, y: e.clientY, start: shown });
  };
  const move = (e: ReactPointerEvent) => {
    const d = drag;
    if (!d || !frame.w) return;
    setLive(dragRect(d.start, d.h, (e.clientX - d.x) / frame.w, (e.clientY - d.y) / frame.h, lock));
  };
  const up = () => {
    if (drag && live) onCommit(live);
    setDrag(null);
  };

  return (
    <div ref={panel} className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-black/95 p-3" data-testid="ed-crop">
      <div className="flex flex-wrap items-center justify-center gap-1.5">
        {CROP_PRESETS.map((p) => (
          <button
            key={p}
            onClick={() => onPreset(p)}
            className={
              "rounded-full border px-2.5 py-1 text-[11px] font-medium " +
              (p === preset
                ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                : "border-white/25 text-white/80 hover:bg-white/10")
            }
          >
            {p === "free" ? t("rec2.ed.crop_free") : p}
          </button>
        ))}
        <button onClick={onReset} className="rounded-full border border-white/25 px-2.5 py-1 text-[11px] text-white/80 hover:bg-white/10">
          {t("rec2.ed.crop_reset")}
        </button>
      </div>
      <div className="relative" style={{ width: frame.w, height: frame.h }} onPointerMove={move} onPointerUp={up} onPointerCancel={up}>
        <video
          ref={video}
          src={videoSrc}
          muted
          preload="auto"
          className="h-full w-full"
          onLoadedMetadata={(e) => {
            e.currentTarget.currentTime = sourceMs / 1000;
          }}
        />
        <div
          className="absolute cursor-move border-2 border-white shadow-[0_0_0_9999px_rgba(0,0,0,0.55)]"
          style={{ left: `${shown.x * 100}%`, top: `${shown.y * 100}%`, width: `${shown.w * 100}%`, height: `${shown.h * 100}%` }}
          onPointerDown={down("move")}
          data-testid="ed-crop-rect"
        >
          {(["nw", "ne", "sw", "se"] as const).map((h) => (
            <span
              key={h}
              onPointerDown={down(h)}
              className={
                "absolute h-3 w-3 rounded-sm border border-black/40 bg-white " +
                (h === "nw" ? "-left-1.5 -top-1.5 cursor-nwse-resize" : "") +
                (h === "ne" ? "-right-1.5 -top-1.5 cursor-nesw-resize" : "") +
                (h === "sw" ? "-bottom-1.5 -left-1.5 cursor-nesw-resize" : "") +
                (h === "se" ? "-bottom-1.5 -right-1.5 cursor-nwse-resize" : "")
              }
            />
          ))}
        </div>
      </div>
      <p className="text-[11px] text-white/60">{t("rec2.ed.crop_hint")}</p>
    </div>
  );
}
