import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { editorToSource, type RecordEditDoc, type RecordEditorView } from "../../lib/ipc";
import { MAX_SPEED, MIN_CLIP_MS, MIN_SPEED, SPEED_STEPS, formatTimecode, setTrim, trimSourceAt } from "../../lib/editor";
import { useT } from "../../lib/i18n";

const fmtSpeed = (s: number) => `${Number(s.toFixed(2))}×`;
/** Upward drag of this many px doubles a region's speed. */
const PX_PER_DOUBLING = 40;

type Drag =
  | {
      kind: "trim";
      clip: number;
      edge: "start" | "end";
      x0: number;
      dx: number;
      span: { progStartMs: number; progEndMs: number };
      bounds: { startMs: number; endMs: number };
      /** The edge's source position as Rust last answered (optimistic label). */
      sourceMs: number;
    }
  | { kind: "speed"; index: number; y0: number; value: number; moved: boolean }
  | { kind: "head" };

/**
 * Timeline: filmstrip, clips (with trim handles), speed band and the playhead. All
 * geometry comes from the programme spans Rust returned (`view`). A trim drag turns the
 * pointer into programme ms and asks Rust for the source position (`editor_to_source`, one
 * request in flight); edits are built from `getDoc()` (the newest document), not `view`.
 */
export function Timeline({
  view,
  thumbs,
  positionMs,
  durationMs,
  selectedClip,
  onSelect,
  onSeek,
  getDoc,
  onChange,
  onSpeedValue,
  onSpeedRemove,
  onWidth,
}: {
  view: RecordEditorView;
  thumbs: string[];
  positionMs: number;
  durationMs: number;
  selectedClip: number | null;
  onSelect: (clip: number) => void;
  onSeek: (programmeMs: number) => void;
  /** The newest document (History's present) — edits must never start from a stale `view`. */
  getDoc: () => RecordEditDoc;
  onChange: (doc: RecordEditDoc) => void;
  onSpeedValue: (index: number, speed: number) => void;
  onSpeedRemove: (index: number) => void;
  onWidth: (px: number) => void;
}) {
  const t = useT();
  const track = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [menu, setMenu] = useState<number | null>(null);
  const total = Math.max(1, view.programmeMs);
  const scale = width / total;
  const doc = view.doc;

  useEffect(() => {
    const el = track.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((e) => {
      const w = e[0]?.contentRect.width ?? 0;
      setWidth(w);
      onWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [onWidth]);

  const progAt = (clientX: number) => {
    const r = track.current?.getBoundingClientRect();
    if (!r || !r.width) return 0;
    return Math.min(total, Math.max(0, ((clientX - r.left) / r.width) * total));
  };

  const resolving = useRef(false);
  const pending = useRef<number | null>(null);
  const edgeProg = (d: Extract<Drag, { kind: "trim" }>) =>
    (d.edge === "start" ? d.span.progStartMs : d.span.progEndMs) + (scale > 0 ? d.dx / scale : 0);
  const resolveTrim = (d: Extract<Drag, { kind: "trim" }>, prog: number) =>
    trimSourceAt(d.clip, d.edge, d.span, d.bounds, prog, (p) => editorToSource(p));
  /** Optimistic source label while dragging: one request in flight, the latest wins. */
  const requestResolve = (d: Extract<Drag, { kind: "trim" }>, prog: number) => {
    if (resolving.current) {
      pending.current = prog;
      return;
    }
    resolving.current = true;
    resolveTrim(d, prog)
      .then((src) => setDrag((cur) => (cur?.kind === "trim" && cur.clip === d.clip ? { ...cur, sourceMs: src } : cur)))
      .catch(() => undefined)
      .finally(() => {
        resolving.current = false;
        const next = pending.current;
        pending.current = null;
        if (next != null) requestResolve(d, next);
      });
  };

  const capture = (e: ReactPointerEvent) => (e.currentTarget as Element).setPointerCapture?.(e.pointerId);

  const onMove = (e: ReactPointerEvent) => {
    if (!drag) return;
    if (drag.kind === "head") onSeek(progAt(e.clientX));
    else if (drag.kind === "trim") {
      const next = { ...drag, dx: e.clientX - drag.x0 };
      setDrag(next);
      requestResolve(next, edgeProg(next));
    }
    else {
      const dy = drag.y0 - e.clientY;
      if (Math.abs(dy) < 3 && !drag.moved) return;
      const base = getDoc().speed[drag.index]?.speed ?? 1;
      const v = Math.min(MAX_SPEED, Math.max(MIN_SPEED, base * 2 ** (dy / PX_PER_DOUBLING)));
      setDrag({ ...drag, value: Math.round(v * 4) / 4 || MIN_SPEED, moved: true });
    }
  };

  const onUp = () => {
    const d = drag;
    setDrag(null);
    if (!d) return;
    if (d.kind === "trim" && d.dx !== 0 && scale > 0) {
      pending.current = null;
      void resolveTrim(d, edgeProg(d))
        .then((src) => {
          const cur = getDoc();
          const c = cur.clips[d.clip];
          if (!c) return;
          if (d.edge === "start") {
            onChange(setTrim(cur, d.clip, Math.min(c.endMs - MIN_CLIP_MS, Math.max(0, src)), c.endMs));
          } else {
            onChange(setTrim(cur, d.clip, c.startMs, Math.max(c.startMs + MIN_CLIP_MS, Math.min(durationMs, src))));
          }
        })
        .catch(() => undefined);
    } else if (d.kind === "speed") {
      if (d.moved) onSpeedValue(d.index, d.value);
      else setMenu(menu === d.index ? null : d.index);
    }
  };

  return (
    <div
      className="flex select-none flex-col gap-1 px-4 pb-3 pt-2"
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={() => setDrag(null)}
    >
      <div
        ref={track}
        className="relative cursor-text"
        onPointerDown={(e) => {
          capture(e);
          setMenu(null);
          setDrag({ kind: "head" });
          onSeek(progAt(e.clientX));
        }}
        data-testid="ed-track"
      >
        {/* Filmstrip */}
        <div className="flex h-12 overflow-hidden rounded-md bg-[var(--color-bg)]">
          {thumbs.map((src, i) => (
            <img key={i} src={src} alt="" draggable={false} className="h-full min-w-0 flex-1 object-cover opacity-80" />
          ))}
        </div>

        {/* Clips */}
        <div className="relative mt-1 h-9">
          {view.clips.map((span, i) => {
            const sel = selectedClip === i;
            let left = span.progStartMs * scale;
            let w = (span.progEndMs - span.progStartMs) * scale;
            if (drag?.kind === "trim" && drag.clip === i) {
              if (drag.edge === "start") {
                left += drag.dx;
                w -= drag.dx;
              } else w += drag.dx;
            }
            return (
              <div
                key={i}
                data-testid="ed-clip"
                onPointerDown={(e) => {
                  e.stopPropagation();
                  onSelect(i);
                }}
                className={
                  "absolute top-0 flex h-full items-center overflow-hidden rounded-md border px-3 text-[11px] tabular-nums " +
                  (sel
                    ? "border-[var(--color-accent)] bg-[var(--color-accent)]/25 text-[var(--color-fg)]"
                    : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]")
                }
                style={{ left, width: Math.max(4, w - 2) }}
                title={`${formatTimecode(doc.clips[i]?.startMs ?? 0)} – ${formatTimecode(doc.clips[i]?.endMs ?? 0)}`}
              >
                <span className="truncate">
                  {drag?.kind === "trim" && drag.clip === i
                    ? formatTimecode(drag.sourceMs)
                    : formatTimecode(span.progEndMs - span.progStartMs)}
                </span>
                {(["start", "end"] as const).map((edge) => (
                  <span
                    key={edge}
                    aria-label={edge === "start" ? t("rec2.ed.trim_start") : t("rec2.ed.trim_end")}
                    onPointerDown={(e) => {
                      e.stopPropagation();
                      capture(e);
                      onSelect(i);
                      const b = doc.clips[i];
                      setDrag({
                        kind: "trim",
                        clip: i,
                        edge,
                        x0: e.clientX,
                        dx: 0,
                        span,
                        bounds: { startMs: b.startMs, endMs: b.endMs },
                        sourceMs: edge === "start" ? b.startMs : b.endMs,
                      });
                    }}
                    className={
                      "absolute top-0 h-full w-2 cursor-ew-resize bg-[var(--color-accent)]/60 opacity-0 transition-opacity duration-(--duration-fast) ease-sharp hover:opacity-100 " +
                      (edge === "start" ? "left-0 rounded-l-md" : "right-0 rounded-r-md")
                    }
                  />
                ))}
              </div>
            );
          })}
        </div>

        {/* Speed band */}
        <div className="relative mt-1 h-5">
          {view.speed.map((span) => {
            const r = doc.speed[span.index];
            if (!r) return null;
            const value = drag?.kind === "speed" && drag.index === span.index && drag.moved ? drag.value : r.speed;
            return (
              <div key={span.index} className="absolute top-0 h-full" style={{ left: span.progStartMs * scale, width: Math.max(14, (span.progEndMs - span.progStartMs) * scale) }}>
                <button
                  data-testid="ed-speed"
                  title={t("rec2.ed.speed_hint")}
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    capture(e);
                    setDrag({ kind: "speed", index: span.index, y0: e.clientY, value: r.speed, moved: false });
                  }}
                  className={
                    "h-full w-full cursor-ns-resize truncate rounded-sm px-1 text-[10px] font-medium tabular-nums " +
                    (value > 1 ? "bg-amber-500/80 text-black" : "bg-sky-500/80 text-black")
                  }
                >
                  {fmtSpeed(value)}
                </button>
                {menu === span.index && (
                  <div
                    className="pop-enter absolute bottom-6 left-0 z-10 flex flex-wrap gap-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1.5 shadow-xl"
                    style={{ width: 196 }}
                    onPointerDown={(e) => e.stopPropagation()}
                  >
                    {SPEED_STEPS.map((s) => (
                      <button
                        key={s}
                        onClick={() => {
                          setMenu(null);
                          onSpeedValue(span.index, s);
                        }}
                        className={
                          "rounded px-1.5 py-0.5 text-[11px] tabular-nums hover:bg-[var(--color-bg)] " +
                          (s === r.speed ? "text-[var(--color-accent)]" : "")
                        }
                      >
                        {fmtSpeed(s)}
                      </button>
                    ))}
                    <button
                      onClick={() => {
                        setMenu(null);
                        onSpeedRemove(span.index);
                      }}
                      className="rounded px-1.5 py-0.5 text-[11px] text-red-500 hover:bg-[var(--color-bg)]"
                    >
                      {t("rec2.ed.speed_remove")}
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Playhead */}
        <div
          className="pointer-events-none absolute -bottom-1 -top-1 w-0.5 bg-red-500"
          style={{ transform: `translateX(${Math.min(total, positionMs) * scale}px)` }}
          data-testid="ed-playhead"
        >
          <span className="absolute -left-[5px] -top-1 h-3 w-3 rounded-full bg-red-500" />
        </div>
      </div>
    </div>
  );
}
