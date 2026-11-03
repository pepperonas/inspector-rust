import { useState, type PointerEvent as ReactPointerEvent } from "react";
import { editorToSource, type RecordAudioTrack, type RecordEditDoc, type RecordEditorView, type RecordWaveSpan } from "../../lib/ipc";
import { updateTrack } from "../../lib/editor";
import { useT } from "../../lib/i18n";

/** Shortest track a drag leaves, programme ms. */
export const MIN_TRACK_MS = 100;
/** A pointer moved less than this many px: a click (select), not a drag. */
const CLICK_SLOP_PX = 3;
/** Height of one track row, px. */
export const LANE_ROW_PX = 26;

/** A track as the lane draws it: the extent of its entries on the programme (Rust's spans). */
export interface TrackBar {
  index: number;
  progStartMs: number;
  progEndMs: number;
  /** Programme ms where a loop repeat after the first begins. */
  repeats: number[];
  muted: boolean;
}

/** One bar per track that has entries, from `EditorView.tracks` (never re-projected here). */
export function trackBars(view: RecordEditorView): TrackBar[] {
  const by = new Map<number, TrackBar>();
  for (const s of view.tracks ?? []) {
    const b = by.get(s.index);
    if (!b) {
      by.set(s.index, { index: s.index, progStartMs: s.progStartMs, progEndMs: s.progEndMs, repeats: s.repeat > 0 ? [s.progStartMs] : [], muted: s.muted });
    } else {
      b.progStartMs = Math.min(b.progStartMs, s.progStartMs);
      b.progEndMs = Math.max(b.progEndMs, s.progEndMs);
      if (s.repeat > 0) b.repeats.push(s.progStartMs);
    }
  }
  return [...by.values()].sort((a, b) => a.index - b.index);
}

export type TrackDragMode = "move" | "start" | "end";

/** Where a drag puts a track: its programme start (Rust maps it to an anchor) and its file window. */
export interface TrackDragResult {
  progStartMs: number;
  /** Where the bar then ends (what the drag shows). */
  progEndMs: number;
  offsetMs: number;
  spanMs: number | null;
}

/**
 * A drag of `deltaMs` on a bar, clamped to the programme (start ≥ 0, end ≤ `programmeMs`):
 * moving keeps the length; the start edge eats into the file (`offsetMs`, never below 0); the
 * end edge sets the played length (`spanMs`, up to the file's end unless the track loops).
 * Tracks play at 1×, so programme ms and file ms move together.
 */
export function dragTrackBar(
  bar: Pick<TrackBar, "progStartMs" | "progEndMs">,
  track: Pick<RecordAudioTrack, "offsetMs" | "spanMs" | "durationMs" | "looped">,
  mode: TrackDragMode,
  deltaMs: number,
  programmeMs: number,
): TrackDragResult {
  const len = bar.progEndMs - bar.progStartMs;
  const d = Number.isFinite(deltaMs) ? deltaMs : 0;
  const same = { progStartMs: bar.progStartMs, progEndMs: bar.progEndMs, offsetMs: track.offsetMs, spanMs: track.spanMs };
  if (mode === "move") {
    const hi = Math.max(0, programmeMs - len);
    const start = Math.round(Math.min(hi, Math.max(0, bar.progStartMs + d)));
    return { ...same, progStartMs: start, progEndMs: start + len };
  }
  if (mode === "start") {
    const lo = Math.max(0, bar.progStartMs - track.offsetMs);
    const hi = Math.max(lo, bar.progEndMs - MIN_TRACK_MS);
    const start = Math.round(Math.min(hi, Math.max(lo, bar.progStartMs + d)));
    const moved = start - bar.progStartMs;
    return {
      ...same,
      progStartMs: start,
      offsetMs: Math.max(0, track.offsetMs + moved),
      spanMs: track.spanMs == null ? null : Math.max(MIN_TRACK_MS, track.spanMs - moved),
    };
  }
  const fileLeft = track.durationMs > 0 && !track.looped ? track.durationMs - track.offsetMs : Infinity;
  const maxEnd = Math.min(programmeMs, bar.progStartMs + fileLeft);
  const end = Math.round(Math.min(maxEnd, Math.max(bar.progStartMs + MIN_TRACK_MS, bar.progEndMs + d)));
  const current = track.spanMs ?? (track.looped || !(track.durationMs > 0) ? len : track.durationMs - track.offsetMs);
  return { ...same, progEndMs: end, spanMs: Math.max(MIN_TRACK_MS, current + (end - bar.progEndMs)) };
}

/** A track's name: its label, else its file name without the extension. */
export function trackName(t: RecordAudioTrack): string {
  if (t.label.trim()) return t.label.trim();
  return t.file.replace(/\.[^.]+$/, "");
}

/**
 * A waveform: `[min, max]` pairs drawn as one filled shape over its box (`viewBox` stretched
 * to the element, so it scales with the timeline).
 */
export function Waveform({ peaks, className }: { peaks: readonly number[]; className?: string }) {
  const n = Math.floor(peaks.length / 2);
  if (n < 2) return null;
  const v = (i: number) => Math.max(-1, Math.min(1, Number.isFinite(peaks[i]) ? peaks[i] : 0));
  const top: string[] = [];
  const lows: string[] = [];
  for (let i = 0; i < n; i += 1) top.push(`${i} ${(-v(2 * i + 1)).toFixed(3)}`);
  for (let i = n - 1; i >= 0; i -= 1) lows.push(`${i} ${(-v(2 * i)).toFixed(3)}`);
  const d = `M${top.join(" L")} L${lows.join(" L")} Z`;
  return (
    <svg className={className} viewBox={`0 -1 ${n - 1} 2`} preserveAspectRatio="none" aria-hidden data-testid="ed-wave">
      <path d={d} fill="currentColor" />
    </svg>
  );
}

/** The waveforms inside a box spanning `startMs..endMs` (programme), positioned by their own spans. */
export function WaveLayer({
  waves,
  startMs,
  endMs,
  className,
}: {
  waves: readonly RecordWaveSpan[];
  startMs: number;
  endMs: number;
  className?: string;
}) {
  const len = endMs - startMs;
  if (!(len > 0) || waves.length === 0) return null;
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden>
      {waves.map((w, i) => (
        <div
          key={i}
          className="absolute inset-y-0"
          style={{ left: `${((w.progStartMs - startMs) / len) * 100}%`, width: `${((w.progEndMs - w.progStartMs) / len) * 100}%` }}
        >
          <Waveform peaks={w.peaks} className={"h-full w-full " + (className ?? "")} />
        </div>
      ))}
    </div>
  );
}

type Drag = { index: number; id: string; mode: TrackDragMode; x0: number; dx: number };

/**
 * The audio lane under the clips: one bar per music/voiceover track, from Rust's programme
 * entries, with its waveform and the loop repeats marked. Dragging the bar moves the track
 * (Rust maps the new programme start to an anchor), its edges trim the file's start
 * (`offsetMs`) and the played length; every drag is clamped to the programme and is one step.
 */
export function AudioLane({
  view,
  scale,
  getDoc,
  onChange,
  onError,
  selected,
  onSelect,
  locked = false,
}: {
  view: RecordEditorView;
  /** px per programme ms. */
  scale: number;
  getDoc: () => RecordEditDoc;
  onChange: (doc: RecordEditDoc) => void;
  onError: (e: unknown) => void;
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** A voiceover take runs: nothing can be dragged. */
  locked?: boolean;
}) {
  const t = useT();
  const [drag, setDrag] = useState<Drag | null>(null);
  const bars = trackBars(view);
  const doc = view.doc;
  if (bars.length === 0) return null;
  const total = Math.max(1, view.programmeMs);

  const begin = (b: TrackBar, mode: TrackDragMode) => (e: ReactPointerEvent) => {
    e.stopPropagation();
    const tr = doc.audioTracks[b.index];
    if (!tr) return;
    onSelect(tr.id);
    if (locked) return;
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* synthetic events */
    }
    setDrag({ index: b.index, id: tr.id, mode, x0: e.clientX, dx: 0 });
  };

  const finish = async (d: Drag) => {
    const bar = bars.find((b) => b.index === d.index);
    const cur = getDoc();
    const tr = cur.audioTracks.find((x) => x.id === d.id);
    if (!bar || !tr || scale <= 0) return;
    const r = dragTrackBar(bar, tr, d.mode, d.dx / scale, total);
    const patch: Partial<RecordAudioTrack> = { offsetMs: r.offsetMs, spanMs: r.spanMs };
    if (r.progStartMs !== bar.progStartMs) {
      // R12: Rust maps the programme start to the anchor (clip + source ms).
      const at = await editorToSource(Math.min(r.progStartMs, Math.max(0, total - 1)));
      if (!at) return;
      patch.clip = at.clip;
      patch.startMs = Math.round(at.sourceMs);
    }
    if (patch.offsetMs === tr.offsetMs && patch.spanMs === tr.spanMs && patch.clip === undefined) return;
    onChange(updateTrack(getDoc(), d.id, patch));
  };

  return (
    <div
      className="relative mt-1"
      style={{ height: LANE_ROW_PX * bars.length }}
      data-testid="ed-audio-lane"
      onPointerMove={(e) => drag && setDrag({ ...drag, dx: e.clientX - drag.x0 })}
      onPointerUp={() => {
        const d = drag;
        setDrag(null);
        if (d && Math.abs(d.dx) >= CLICK_SLOP_PX) void finish(d).catch(onError);
      }}
      onPointerCancel={() => setDrag(null)}
    >
      {bars.map((b, row) => {
        const tr = doc.audioTracks[b.index];
        if (!tr) return null;
        const sel = selected === tr.id;
        const d = drag?.index === b.index ? drag : null;
        const shown = d && scale > 0 ? dragTrackBar(b, tr, d.mode, d.dx / scale, total) : b;
        const start = shown.progStartMs;
        const end = shown.progEndMs;
        const waves = (view.waveforms ?? []).filter((w) => w.kind === "track" && w.track === b.index);
        const music = tr.kind === "music";
        const kindLabel = music ? t("rec4.audio.kind_music") : t("rec4.audio.kind_voiceover");
        return (
          <div
            key={tr.id}
            data-testid="ed-audio-bar"
            data-kind={tr.kind}
            data-muted={tr.muted ? "" : undefined}
            aria-selected={sel}
            title={`${kindLabel} · ${trackName(tr)}`}
            onPointerDown={begin(b, "move")}
            className={
              "absolute flex items-center overflow-hidden rounded-sm border px-1.5 text-[10px] font-medium " +
              (locked ? "cursor-default " : "cursor-grab ") +
              (music ? "border-teal-500 bg-teal-700/80 text-white " : "border-rose-500 bg-rose-700/80 text-white ") +
              (tr.muted ? "opacity-45 " : "") +
              (sel ? "ring-2 ring-[var(--color-accent)]" : "")
            }
            style={{ left: start * scale, top: LANE_ROW_PX * row, height: LANE_ROW_PX - 4, width: Math.max(10, (end - start) * scale) }}
          >
            <WaveLayer waves={waves} startMs={b.progStartMs} endMs={b.progEndMs} className={music ? "text-teal-200/60" : "text-rose-200/60"} />
            {b.repeats.map((ms) => (
              <span
                key={ms}
                data-testid="ed-audio-repeat"
                aria-hidden
                className="pointer-events-none absolute inset-y-0 w-px border-l border-dashed border-white/70"
                style={{ left: (ms - b.progStartMs) * scale }}
              />
            ))}
            <span className="relative truncate">{trackName(tr)}</span>
            {!locked &&
              (["start", "end"] as const).map((edge) => (
                <span
                  key={edge}
                  aria-label={edge === "start" ? t("rec4.audio.trim_start") : t("rec4.audio.trim_end")}
                  data-testid={`ed-audio-${edge}`}
                  onPointerDown={begin(b, edge)}
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
  );
}
