import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { editorToSource, type RecordEditDoc, type RecordEditorView } from "../../lib/ipc";
import {
  MAX_SPEED,
  MIN_CLIP_MS,
  MIN_SPEED,
  SPEED_STEPS,
  addZoom,
  fmtZoom,
  formatTimecode,
  isTypingTarget,
  moveZoomProg,
  removeZoom,
  setTrim,
  trimSourceAt,
  trimZoomProg,
  updateZoom,
  zoomScale,
  zoomSourceAt,
} from "../../lib/editor";
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
  | {
      kind: "zoom";
      id: string;
      mode: "move" | "start" | "end";
      x0: number;
      dx: number;
      span: { progStartMs: number; progEndMs: number };
      clipSpan: { progStartMs: number; progEndMs: number };
    }
  | { kind: "head" };

/** A pointer moved less than this many px: a click (select), not a drag. */
const CLICK_SLOP_PX = 3;

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
  onError,
  onSpeedValue,
  onSpeedRemove,
  onWidth,
  selectedZoom = null,
  onSelectZoom = () => undefined,
  keysEnabled = true,
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
  /** A failed source lookup (zoom drag) goes to the editor's notice path. */
  onError: (e: unknown) => void;
  onSpeedValue: (index: number, speed: number) => void;
  onSpeedRemove: (index: number) => void;
  onWidth: (px: number) => void;
  /** The zoom region (`id`) the inspector edits. */
  selectedZoom?: string | null;
  onSelectZoom?: (id: string | null) => void;
  /** Z (new zoom) and Delete (selected zoom) — off while a dialog or crop mode owns the keys. */
  keysEnabled?: boolean;
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

  // Latest values for the window key handler (registered once per enable flip).
  const live = useRef({ positionMs, selectedZoom, getDoc, onChange, onSelectZoom });
  useEffect(() => {
    live.current = { positionMs, selectedZoom, getDoc, onChange, onSelectZoom };
  });
  useEffect(() => {
    if (!keysEnabled) return;
    // Registered before the editor's own handler (a child's effect runs first): a key this
    // track uses is `preventDefault`ed and the editor leaves it alone.
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.repeat || e.metaKey || e.ctrlKey || e.altKey || isTypingTarget(e.target)) return;
      const cur = live.current;
      if (e.key.toLowerCase() === "z") {
        e.preventDefault();
        void editorToSource(cur.positionMs)
          .then((at) => {
            if (!at) return;
            const doc = cur.getDoc();
            const next = addZoom(doc, at.clip, at.sourceMs);
            const made = next.zooms[next.zooms.length - 1];
            if (next === doc || !made) return;
            cur.onChange(next);
            cur.onSelectZoom(made.id);
          })
          .catch(() => undefined);
      } else if ((e.key === "Delete" || e.key === "Backspace") && cur.selectedZoom) {
        e.preventDefault();
        cur.onChange(removeZoom(cur.getDoc(), cur.selectedZoom));
        cur.onSelectZoom(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keysEnabled]);

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

  const capture = (e: ReactPointerEvent) => {
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* no active pointer (synthetic events) */
    }
  };

  /** The block a zoom drag shows right now (programme ms). */
  const zoomPreview = (d: Extract<Drag, { kind: "zoom" }>) => {
    const delta = scale > 0 ? d.dx / scale : 0;
    return d.mode === "move" ? moveZoomProg(d.span, d.clipSpan, delta) : trimZoomProg(d.span, d.clipSpan, d.mode, delta);
  };

  /** Commits a zoom drag: programme edges → source through Rust, inside the zoom's clip. */
  const commitZoom = async (d: Extract<Drag, { kind: "zoom" }>) => {
    const doc = getDoc();
    const z = doc.zooms.find((x) => x.id === d.id);
    const c = z ? doc.clips[z.clip] : undefined;
    if (!z || !c) return;
    const target = zoomPreview(d);
    const at = (prog: number) => zoomSourceAt(z.clip, d.clipSpan, c, prog, (p) => editorToSource(p));
    let startMs = z.startMs;
    let endMs = z.endMs;
    if (d.mode === "move") {
      startMs = await at(target.progStartMs);
      endMs = Math.min(c.endMs, startMs + (z.endMs - z.startMs));
      startMs = Math.max(c.startMs, endMs - (z.endMs - z.startMs));
    } else if (d.mode === "start") startMs = await at(target.progStartMs);
    else endMs = await at(target.progEndMs);
    if (startMs === z.startMs && endMs === z.endMs) return;
    // A planned zoom the user moved or trimmed is theirs now: "Remove auto-zooms" keeps it.
    onChange(updateZoom(getDoc(), d.id, { startMs, endMs, origin: "manual" }));
  };

  const onMove = (e: ReactPointerEvent) => {
    if (!drag) return;
    if (drag.kind === "head") onSeek(progAt(e.clientX));
    else if (drag.kind === "zoom") setDrag({ ...drag, dx: e.clientX - drag.x0 });
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
    } else if (d.kind === "zoom") {
      if (Math.abs(d.dx) >= CLICK_SLOP_PX && scale > 0) void commitZoom(d).catch(onError);
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

        {/* Zoom track */}
        <div className="relative mt-1 h-5" data-testid="ed-zoom-track">
          {view.zooms.map((span) => {
            const z = doc.zooms[span.index];
            const clipSpan = z ? view.clips[z.clip] : undefined;
            if (!z || !clipSpan) return null;
            const d = drag?.kind === "zoom" && drag.id === z.id ? drag : null;
            const g = d ? zoomPreview(d) : span;
            const sel = selectedZoom === z.id;
            const auto = z.origin === "auto";
            const start = (mode: "move" | "start" | "end") => (e: ReactPointerEvent) => {
              e.stopPropagation();
              capture(e);
              onSelectZoom(z.id);
              setDrag({ kind: "zoom", id: z.id, mode, x0: e.clientX, dx: 0, span, clipSpan });
            };
            return (
              <div
                key={z.id}
                data-testid="ed-zoom"
                data-origin={z.origin}
                aria-selected={sel}
                title={`${t(auto ? "rec3.zoom.auto_block" : "rec3.zoom.manual_block")} · ${fmtZoom(zoomScale(z))}`}
                onPointerDown={start("move")}
                className={
                  "absolute top-0 flex h-full cursor-grab items-center overflow-hidden rounded-sm border px-1.5 text-[10px] font-medium tabular-nums " +
                  (auto
                    ? "border-dashed border-violet-400/80 bg-violet-500/25 text-violet-100"
                    : "border-violet-400 bg-violet-500/70 text-white") +
                  (sel ? " ring-2 ring-[var(--color-accent)]" : "")
                }
                style={{ left: g.progStartMs * scale, width: Math.max(10, (g.progEndMs - g.progStartMs) * scale) }}
              >
                <span className="truncate">{fmtZoom(zoomScale(z))}</span>
                {(["start", "end"] as const).map((edge) => (
                  <span
                    key={edge}
                    aria-label={edge === "start" ? t("rec3.zoom.trim_start") : t("rec3.zoom.trim_end")}
                    onPointerDown={start(edge)}
                    className={
                      "absolute top-0 h-full w-1.5 cursor-ew-resize bg-white/60 opacity-0 transition-opacity duration-(--duration-fast) ease-sharp hover:opacity-100 " +
                      (edge === "start" ? "left-0" : "right-0")
                    }
                  />
                ))}
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
