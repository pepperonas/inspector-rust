import type { PointerEvent as ReactPointerEvent } from "react";
import { focusFromPoint } from "../../lib/editor";
import { useT } from "../../lib/i18n";

/**
 * Fixed zoom focus: a crosshair over the clip's screen box in the preview (`rect`, fractions of
 * the output — Rust's layout, never re-derived here). A click inside the preview places the
 * focus (clamped into the screen).
 */
export function FocusOverlay({
  rect,
  focus,
  onPick,
}: {
  rect: [number, number, number, number];
  focus: { x: number; y: number };
  onPick: (x: number, y: number) => void;
}) {
  const t = useT();
  const [x, y, w, h] = rect;
  const pick = (e: ReactPointerEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const f = focusFromPoint(rect, (e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height);
    onPick(f.x, f.y);
  };
  const pct = (v: number) => `${v * 100}%`;
  return (
    <div
      className="absolute inset-0 cursor-crosshair"
      onPointerDown={pick}
      role="button"
      aria-label={t("rec3.zoom.focus_overlay")}
      data-testid="ed-focus-overlay"
    >
      <div
        className="pointer-events-none absolute border border-dashed border-white/60"
        style={{ left: pct(x), top: pct(y), width: pct(w), height: pct(h) }}
      >
        <div className="absolute inset-y-0 w-px bg-violet-400/90" style={{ left: pct(focus.x) }} />
        <div className="absolute inset-x-0 h-px bg-violet-400/90" style={{ top: pct(focus.y) }} />
        <div
          className="absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white bg-violet-500 shadow"
          style={{ left: pct(focus.x), top: pct(focus.y) }}
        />
      </div>
    </div>
  );
}
